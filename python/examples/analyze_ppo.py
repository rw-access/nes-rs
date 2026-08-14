"""Analyze PPO action outputs and RAM-byte attribution along an episode trace."""

from __future__ import annotations

import argparse
import json
import sys
from pathlib import Path

import numpy as np

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "src"))

from nes_gym import SuperMarioBros1_1Env
from nes_gym.trace import EpisodeTrace, REWIND_INPUT


ACTION_LABELS = ("NOOP", "LEFT", "RIGHT", "A", "LEFT+A", "RIGHT+A", "REWIND")
KNOWN_RAM = {
    0x000E: "level state",
    0x001D: "flag state",
    0x006D: "world X high",
    0x0086: "world X low",
    0x00B5: "Y viewport",
    0x075A: "lives",
    0x075C: "stage",
    0x075F: "world",
}


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("rom", type=Path)
    parser.add_argument("model", type=Path)
    parser.add_argument("trace", type=Path)
    parser.add_argument("output", type=Path)
    parser.add_argument("--library", default=None)
    parser.add_argument("--samples", type=int, default=400)
    parser.add_argument("--top-addresses", type=int, default=64)
    parser.add_argument("--max-episode-frames", type=int, default=3600)
    args = parser.parse_args()
    if args.samples <= 0 or args.top_addresses <= 0:
        parser.error("samples and top-addresses must be positive")

    try:
        import torch
        from stable_baselines3 import PPO
        from stable_baselines3.common.utils import obs_as_tensor
    except (ImportError, OSError) as exc:
        raise SystemExit(f"PyTorch/Stable-Baselines3 could not be imported: {exc}") from exc

    from train_ppo import NormalizeRamObservation

    trace = EpisodeTrace.from_json(args.trace.read_text(encoding="utf-8"))
    args.output.parent.mkdir(parents=True, exist_ok=True)

    observations: list[np.ndarray] = []
    actual_actions: list[int] = []
    world_x: list[int] = []
    scores: list[int] = []
    delta_x: list[int] = []

    with SuperMarioBros1_1Env(
        args.rom,
        library=args.library,
        max_episode_frames=args.max_episode_frames,
        death_penalty=100.0,
        completion_bonus=1000.0,
        score_coef=0.1,
    ) as base_env:
        env = NormalizeRamObservation(base_env)
        model = PPO.load(str(args.model), env=env, device="cpu")
        model.policy.set_training_mode(False)
        base_env.reset(seed=0)
        action_mapping = tuple(base_env.action_mapping)

        previous_metrics = base_env.metrics()
        for controller, count in trace.inputs_rle:
            for _ in range(count):
                observations.append(np.asarray(base_env.ram, dtype=np.float32).copy() / 255.0)
                if controller == REWIND_INPUT:
                    actual_actions.append(len(action_mapping))
                    base_env.core.rewind()
                else:
                    actual_actions.append(action_mapping.index(controller))
                    base_env.core.advance_frames(controller, 1)
                current_metrics = base_env.metrics()
                world_x.append(int(previous_metrics["world_x"]))
                scores.append(int(previous_metrics["score"]))
                delta_x.append(int(current_metrics["world_x"]) - int(previous_metrics["world_x"]))
                previous_metrics = current_metrics

        obs_array = np.asarray(observations, dtype=np.float32)
        sample_indices = np.unique(
            np.linspace(0, len(obs_array) - 1, min(args.samples, len(obs_array)), dtype=np.int64)
        )
        obs_tensor = obs_as_tensor(obs_array, model.device).detach().requires_grad_(True)
        features = model.policy.extract_features(obs_tensor, model.policy.pi_features_extractor)
        latent_pi, latent_vf = model.policy.mlp_extractor(features)
        logits = model.policy.action_net(latent_pi)
        probabilities = torch.softmax(logits, dim=1).detach().cpu().numpy()
        values = model.policy.value_net(latent_vf).detach().cpu().numpy().reshape(-1)

        top_two = logits.detach().topk(2, dim=1).indices
        margin = logits.gather(1, top_two[:, :1]).squeeze(1) - logits.gather(1, top_two[:, 1:2]).squeeze(1)
        margin.sum().backward()
        contribution = (obs_tensor.grad * obs_tensor).abs().detach().cpu().numpy()

        # This is an attribution proxy: mean absolute input-gradient times
        # input value for the model's top-vs-runner-up action margin.
        address_scores = contribution.mean(axis=0)
        address_order = np.argsort(address_scores)[::-1]
        top_addresses = address_order[: min(args.top_addresses, len(address_order))]
        heat_addresses = top_addresses[:32]
        heat_scale = float(max(contribution[:, heat_addresses].max(), 1e-12))

        # Validate the gradient ranking with a direct counterfactual on the
        # most important sampled addresses. Replacing one normalized byte by
        # 0.5 asks how much the policy output changes without pretending the
        # resulting RAM state is physically valid.
        counterfactual_stats: dict[int, dict[str, float]] = {}
        sampled_obs = obs_array[sample_indices]
        sampled_probs = probabilities[sample_indices]
        sampled_top = np.argmax(sampled_probs, axis=1)
        for address in top_addresses[:24]:
            counterfactual_obs = sampled_obs.copy()
            counterfactual_obs[:, address] = 0.5
            counterfactual_tensor = obs_as_tensor(counterfactual_obs, model.device)
            with torch.no_grad():
                cf_features = model.policy.extract_features(
                    counterfactual_tensor, model.policy.pi_features_extractor
                )
                cf_latent_pi, _ = model.policy.mlp_extractor(cf_features)
                cf_logits = model.policy.action_net(cf_latent_pi)
                cf_probs = torch.softmax(cf_logits, dim=1).cpu().numpy()
            safe_probs = np.clip(sampled_probs, 1e-8, 1.0)
            safe_cf_probs = np.clip(cf_probs, 1e-8, 1.0)
            kl = np.sum(safe_probs * (np.log(safe_probs) - np.log(safe_cf_probs)), axis=1)
            counterfactual_stats[int(address)] = {
                "mean_kl": float(np.mean(kl)),
                "max_kl": float(np.max(kl)),
                "top_action_flip_rate": float(np.mean(np.argmax(cf_probs, axis=1) != sampled_top)),
            }

        action_mapping_labels = [
            ACTION_LABELS[index] if index < len(ACTION_LABELS) else f"ACTION_{index}"
            for index in range(len(action_mapping) + 1)
        ]
        attention = [
            {
                "address": int(address),
                "hex": f"0x{int(address):04X}",
                "mean_attribution": float(address_scores[address]),
                "max_attribution": float(contribution[:, address].max()),
                "counterfactual": counterfactual_stats.get(int(address)),
            }
            for address in top_addresses
        ]
        known_addresses = []
        for address, label in KNOWN_RAM.items():
            rank = int(np.flatnonzero(address_order == address)[0]) + 1
            known_addresses.append(
                {
                    "address": address,
                    "hex": f"0x{address:04X}",
                    "label": label,
                    "rank": rank,
                    "mean_attribution": float(address_scores[address]),
                }
            )
        timeline = []
        for index in sample_indices:
            index = int(index)
            top = int(np.argmax(probabilities[index]))
            actual = int(actual_actions[index])
            timeline.append(
                {
                    "frame": index + 1,
                    "world_x": world_x[index],
                    "score": scores[index],
                    "delta_x": delta_x[index],
                    "value": float(values[index]),
                    "actual": actual,
                    "actual_label": action_mapping_labels[actual],
                    "top": top,
                    "top_label": action_mapping_labels[top],
                    "probabilities": [float(value) for value in probabilities[index]],
                }
            )

        address_values = {
            str(int(address)): [int(round(float(obs_array[index, address] * 255.0))) for index in sample_indices]
            for address in heat_addresses
        }
        address_heat = {
            str(int(address)): [float(contribution[index, address] / heat_scale) for index in sample_indices]
            for address in heat_addresses
        }

    result = {
        "model": str(args.model),
        "trace": str(args.trace),
        "frames": len(obs_array),
        "sample_indices": [int(index) + 1 for index in sample_indices],
        "action_labels": action_mapping_labels,
        "attention_method": "mean absolute input-gradient times normalized input for top-vs-runner-up action margin",
        "attention": attention,
        "known_addresses": known_addresses,
        "timeline": timeline,
        "address_values": address_values,
        "address_heat": address_heat,
    }
    args.output.write_text(json.dumps(result, separators=(",", ":")) + "\n", encoding="utf-8")
    print(args.output)


if __name__ == "__main__":
    main()

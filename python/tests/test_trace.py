from nes_gym.trace import CheckpointRef, EpisodeTrace, EpisodeTraceBuilder, REWIND_INPUT, expand_rle
from nes_gym.metadata import experiment_metadata


def test_rle_merges_and_expands_exact_frames():
    builder = EpisodeTraceBuilder(CheckpointRef.root("root"))
    builder.append(3, 2)
    builder.append(3, 4)
    builder.append(7, 1)
    trace = builder.finish()
    assert trace.inputs_rle == ((3, 6), (7, 1))
    assert list(trace.inputs()) == [3, 3, 3, 3, 3, 3, 7]
    assert trace.episode_frames == 7


def test_trace_serialization_contains_provenance():
    trace = EpisodeTrace(CheckpointRef.derived(CheckpointRef.root("r"), [(1, 5)]), ((2, 3),))
    data = trace.to_dict()
    assert data["checkpoint"]["parent"] == "r"
    assert data["episode_frames"] == 3
    assert list(expand_rle(data["inputs_rle"])) == [2, 2, 2]


def test_rewind_input_is_serializable_and_separate_from_controller_bits():
    trace = EpisodeTrace(CheckpointRef.root("root"), ((REWIND_INPUT, 2),))
    assert trace.to_dict()["inputs_rle"] == [[REWIND_INPUT, 2]]
    assert list(trace.inputs()) == [REWIND_INPUT, REWIND_INPUT]


def test_experiment_metadata_identifies_reproduction_configuration():
    class Core:
        rom_sha256 = "rom-hash"
        library_path = "nes_ffi.dll"

    class Env:
        core = Core()
        init_sequence = ((0, 60), (8, 1), (0, 105))
        root_checkpoint = CheckpointRef.root("root-id")
        action_mapping = (0, 64, 128)
        frame_skip = 1

    metadata = experiment_metadata(Env())
    assert metadata["rom_sha256"] == "rom-hash"
    assert metadata["root_checkpoint"] == "root-id"
    assert metadata["initialization_sequence"] == [[0, 60], [8, 1], [0, 105]]

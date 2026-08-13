from nes_gym.trace import CheckpointRef, EpisodeTrace, EpisodeTraceBuilder, expand_rle


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

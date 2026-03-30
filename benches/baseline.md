# jlens Baseline Benchmarks

Date: 2026-03-30
Binary: target/release/jlens (commit dc065ed)
Platform: macOS Darwin 25.2.0, Apple Silicon

## Parse Performance

| Fixture | Size | Nodes | Strategy | Parse Time |
|---------|------|-------|----------|------------|
| small_1kb.json | 1.2 KB | 51 | full | <1ms |
| medium_100kb.json | 102 KB | 4,201 | full | 1ms |
| medium_1mb.json | 1.0 MB | 42,001 | full | 8ms |
| large_10mb.json | 10.0 MB | 420,001 | full | 83ms |
| large_100mb.json | 99.8 MB | 4,200,001 | full | 1,335ms |
| wide_100k_keys.json | 2.8 MB | 100,001 | full | 31ms |
| deep_500.json | 6 KB | N/A | FAIL | serde_json recursion limit exceeded |

## Observations

1. **Linear scaling**: ~13ms per MB for full parse. Consistent.
2. **100MB in 1.3s**: Acceptable for full parse. Lazy mode threshold is 500MB.
3. **Deep nesting**: serde_json has a default recursion limit of 128. The deep_500.json fixture (500 levels) crashes. This is a known limitation — our arena builder uses serde_json::Value as input.
4. **No lazy benchmarks yet**: All fixtures are under the 500MB lazy threshold. Need a larger fixture (or lower the threshold) to benchmark lazy mode.
5. **Memory not measured here**: Use `/usr/bin/time -l` for peak RSS.

## Targets (Phase 0 completion)

| Fixture | Current | Target |
|---------|---------|--------|
| 10MB | 83ms | <80ms (maintain) |
| 100MB | 1,335ms | <1,200ms (10% faster with arena improvements) |
| 1GB (TBD) | N/A | <5s with lazy mode |
| Deep nesting | CRASH | Handle gracefully (iterative parser or raised limit) |
| Idle CPU at 33ms tick | N/A | <1% (dirty-flag render) |

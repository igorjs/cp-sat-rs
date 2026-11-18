# Proto files

Two files from `google/or-tools`, copied byte for byte from commit `c931f5f778c88050a3f71d72dfc30d3d020aa821`. `crates/proto/build.rs` compiles them with `protox`, so the build needs no `protoc`. `scripts/fetch-upstream.sh` fails when either file differs from the fetched source.

| File | SHA-256 |
|---|---|
| `ortools/sat/cp_model.proto` | `e9814845020b88a55106c7b11d5765067d892af393704ca5eb7f9e7569471a03` |
| `ortools/sat/sat_parameters.proto` | `f9e7a36a80d098b824d45d4865fd6ae366244fe87564ffae5b965c31ffcdeca0` |

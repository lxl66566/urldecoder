# fuzz

```sh
cargo fuzz run fuzz_target_safe --features safe -- -max_len=131072 -jobs=32
cargo fuzz run fuzz_target_unsafe -- -max_len=131072 -jobs=32
```

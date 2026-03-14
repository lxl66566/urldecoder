# urldecoder

English | [简体中文](./docs/README.zh-CN.md)

A CLI tool for batch finding and decoding URLs in text/files, also usable as a Rust library. Highly performance-optimized.

Decoding can shorten string length and improve source code readability, making it ideal for use in blogs, articles, and documentation. For example:

```diff
- https://github.com/lxl66566/my-college-files/tree/main/%E4%BF%A1%E6%81%AF%E7%A7%91%E5%AD%A6%E4%B8%8E%E5%B7%A5%E7%A8%8B%E5%AD%A6%E9%99%A2/%E5%B5%8C%E5%85%A5%E5%BC%8F%E7%B3%BB%E7%BB%9F
+ https://github.com/lxl66566/my-college-files/tree/main/信息科学与工程学院/嵌入式系统
```

## Usage

### Command Line

You can download the corresponding executable for your platform from the [Releases](https://github.com/lxl66566/urldecoder/releases) page.

```sh
Usage: urldecoder [OPTIONS] <FILES>...

Arguments:
  <FILES>...  Input files, supports wildcard globbing

Options:
  -d, --dry-run            Only simulate the operation, do not modify files
  -n, --no-output          Do not print decoded results to the console
  -e, --exclude <EXCLUDE>  Exclude files or directories; prefix matching on relative paths, does not support wildcards
      --escape-space       Do not decode `%20` into spaces; Markdown-friendly
  -h, --help               Print help
  -V, --version            Print version

Examples:
urldecoder test/t.md        # Decode test/t.md
urldecoder *.md -e my.md    # Decode all `.md` files in the current directory, except `my.md`
urldecoder **/*             # Decode all files in the current directory and its subdirectories
```

- By default, the `node_modules` folder is excluded.

My typical usage:

```sh
urldecoder -e src/.vuepress/.cache -e src/.vuepress/.temp -e src/.vuepress/dist --escape-space 'src/**/*.md' 'src/.vuepress/components/*' 'src/.vuepress/data/*'
```

### Rust Library

See the documentation at [docs.rs](https://docs.rs/urldecoder).

Features:

- `bin`: Used for compiling the CLI; enables Rayon parallel decoding + glob file matching.
- `verbose-log`: Enables verbose logging during decoding; may increase buffer copy operations.
- `safe` (default): Atomic write file contents to ensure integrity. Has no effect on in-memory decoding.

## Benchmark

Test environment: Ryzen 7950x, NixOS

String/text content: 90% ordinary ASCII text + 10% URL-encoded strings.

<!-- prettier-ignore -->
| Use Case                                      | unsafe   | safe     |
|-----------------------------------------------|----------|----------|
| Single-threaded (std::io::sink)                | 9.4580 GiB/s | -        |
| Single-threaded (In place)                     | 7.8869 GiB/s | -        |
| Single-file 32KB decode (dry run, read only)   | 3.6112 GiB/s | -        |
| Single-file 32KB decode (RW, tmpfs)            | 1.4933 GiB/s | 1.1948 GiB/s |
| Single-file 10MB decode (dry run, read only)   | 6.6144 GiB/s | -        |
| Single-file 10MB decode (RW, tmpfs)            | 5.7140 GiB/s | 2.1883 GiB/s |
| Parallel 32KB files decode (dry run, read only)| 25.460 GiB/s | -        |
| Parallel 32KB files decode (RW, tmpfs)         | 28.930 GiB/s | 25.808 GiB/s |
| Parallel 4MB files decode (dry run, read only) | 27.133 GiB/s | -        |
| Parallel 4MB files decode (RW, tmpfs)          | 21.860 GiB/s | 11.954 GiB/s |

```sh
cargo bench --bench single_thread --no-default-features
cargo bench --bench single_file --no-default-features
cargo bench --bench multi_files --no-default-features -F bin
```

## Fuzz

```sh
cargo fuzz run fuzz_target_unsafe --no-default-features -- -max_len=131072 -jobs=32
```

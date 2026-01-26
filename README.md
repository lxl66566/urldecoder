# urldecoder

English | [简体中文](./docs/README.zh-CN.md)

CLI tool and Rust library for batch URL decoding. Blazing fast.

Decoding shortens string length and improves readability, especially in blogs, posts and documents. For example:

```diff
- https://github.com/lxl66566/my-college-files/tree/main/%E4%BF%A1%E6%81%AF%E7%A7%91%E5%AD%A6%E4%B8%8E%E5%B7%A5%E7%A8%8B%E5%AD%A6%E9%99%A2/%E5%B5%8C%E5%85%A5%E5%BC%8F%E7%B3%BB%E7%BB%9F
+ https://github.com/lxl66566/my-college-files/tree/main/信息科学与工程学院/嵌入式系统
```

## Usage

### CLI

```sh
Usage: urldecoder.exe [OPTIONS] <FILES>...

Arguments:
  <FILES>...  Files to process, allows wildcard pattern

Options:
  -d, --dry-run            Show result only, without overwrite
  -n, --no-output          Do not print decode result to console
  -e, --exclude <EXCLUDE>  Exclude file or folder by relative path prefix
      --escape-space       Do not decode `%20` to space
  -h, --help               Print help
  -V, --version            Print version

Examples:
urldecoder test/t.md        # Decode test/t.md
urldecoder *.md -e my.md    # Decode all .md files, excluding my.md
urldecoder **/*             # Decode all files recursively
```

The `node_modules` folder is excluded by default.

A real-world usage example:

```sh
urldecoder -e src/.vuepress/.cache -e src/.vuepress/.temp -e src/.vuepress/dist --escape-space 'src/**/*.md' 'src/.vuepress/components/*' 'src/.vuepress/data/*'
```

### Rust Library

Visit [docs.rs](https://docs.rs/urldecoder) for documentation.

Features:

- `bin`: For CLI compilation; enables rayon parallel decoding + glob matching.
- `verbose-log`: Enables logging during decoding.
- `safe` (default): If the decoded URL is not valid UTF-8, do not decode it.

safe mode and verbose-log will copy data in memory more times.

Limits:

- Maximum size for a single URL is 64KB.

## Benchmark

Environment: Ryzen 7950x, NixOS
Content: 90% ASCII text + 10% URL strings

### Single Thread

Pure in-memory URL decoding test.

`cargo bench --bench single_thread --no-default-features`

Result: **3.2944 GiB/s**

### Parallel File Decoding

Multi-threaded file decoding test on tmpfs.

`cargo bench --bench multi_files --no-default-features -F bin`

- 900KB files: **26.370 GiB/s**
- 10MB files: **33.432 GiB/s**

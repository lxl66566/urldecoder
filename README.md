# urldecoder

English | [简体中文](./docs/README.zh-CN.md)

A tool to batch decode URLs in your files. A toy project written in Rust.

Decoding URLs shortens the string length and increases readability. Example:

```
- https://github.com/lxl66566/my-college-files/tree/main/%E4%BF%A1%E6%81%AF%E7%A7%91%E5%AD%A6%E4%B8%8E%E5%B7%A5%E7%A8%8B%E5%AD%A6%E9%99%A2/%E5%B5%8C%E5%85%A5%E5%BC%8F%E7%B3%BB%E7%BB%9F
+ https://github.com/lxl66566/my-college-files/tree/main/信息科学与工程学院/嵌入式系统
```

## Install

### All platforms

Download binary from [Release](https://github.com/lxl66566/urldecoder/releases).

### Windows

In addition to the above methods, you can also install it via [scoop](https://scoop.sh/):

```sh
scoop install https://raw.githubusercontent.com/lxl66566/urldecoder/main/urldecoder.json
```

## Usage

```sh
Usage: urldecoder [OPTIONS] <FILES>...

Arguments:
  <FILES>...  Files to convert, uses glob("{file}") to parse given pattern

Options:
  -d, --dry-run            Show result only, without overwrite
  -v, --verbose            Show full debug and error message
  -e, --exclude <EXCLUDE>  Exclude file or folder
      --escape-space       Do not decode `%20` to space
  -h, --help               Print help
  -V, --version            Print version

Examples:
urldecoder test/t.md        # decode test/t.md
urldecoder *.md -e my.md    # decode all markdown files in current folder except `my.md`
urldecoder **/*             # decode all files recursively in current folder
```

and exclude `node_modules` by default.

Real example of how I use it:

```sh
urldecoder -e src/.vuepress/.cache -e src/.vuepress/.temp -e src/.vuepress/dist --escape-space 'src/**/*.md'
```

to auto decode my vuepress blog before committing.

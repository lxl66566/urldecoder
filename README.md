# urldecoder

A tool to decode URLs in your file. A toy project written in Rust.

for example:

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
urldecoder test/t.md    # decode test/t.md
urldecoder *.md -e my   # decode all markdown files in current folder except which in `my` folder
urldecoder *            # decode all files in current folder
```

more infomation:

```sh
urldecoder -h
```

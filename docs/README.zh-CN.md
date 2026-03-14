# urldecoder

[English](../README.md) | 简体中文

cli 工具，寻找并解码文件中的 URL，也可以作为 rust lib 使用。专注于性能优化。

解码可以缩短字符串长度，增加源码可读性，非常适合用于博客、文章和文档中。例如：

```diff
- https://github.com/lxl66566/my-college-files/tree/main/%E4%BF%A1%E6%81%AF%E7%A7%91%E5%AD%A6%E4%B8%8E%E5%B7%A5%E7%A8%8B%E5%AD%A6%E9%99%A2/%E5%B5%8C%E5%85%A5%E5%BC%8F%E7%B3%BB%E7%BB%9F
+ https://github.com/lxl66566/my-college-files/tree/main/信息科学与工程学院/嵌入式系统
```

## 使用方法

### 命令行

可以在 [Release](https://github.com/lxl66566/urldecoder/releases) 下载对应版本的可执行文件。

```sh
Usage: urldecoder [OPTIONS] <FILES>...

Arguments:
  <FILES>...  传入的文件，支持 wildcard 匹配

Options:
  -d, --dry-run            仅测试运行结果，不修改文件
  -n, --no-output          不在命令行输出解码结果
  -e, --exclude <EXCLUDE>  排除文件或文件夹，相对路径的前缀匹配，不支持 wildcard
      --escape-space       不将 `%20` 解码为空格，markdown 友好
  -h, --help               打印帮助
  -V, --version            打印版本

Examples:
urldecoder test/t.md        # 解码 test/t.md
urldecoder *.md -e my.md    # 解码当前文件夹下所有 `.md` 结尾的文件，除了 `my.md`
urldecoder **/*             # 解码当前文件夹及其子文件夹的所有文件
```

默认情况下将排除 `node_modules` 文件夹。

我的用例：

```sh
urldecoder -e src/.vuepress/.cache -e src/.vuepress/.temp -e src/.vuepress/dist --escape-space 'src/**/*.md' 'src/.vuepress/components/*' 'src/.vuepress/data/*'
```

### rust 库

前往 [docs.rs](https://docs.rs/urldecoder) 查看文档。

features:

- `bin`: 用于编译 cli 程序，启用 rayon 并行解码 + glob 文件匹配。
- `verbose-log`: 启用解码过程中的提示信息输出，buffer 拷贝次数会增多。
- `safe` (default): 原子化写入文件内容，保证文件完整性；对纯内存的解码无影响。

## benchmark

测试环境：Ryzen 7950x, NixOS

字符串/文本内容：90% 的 ASCII 普通文本 + 10% 的 URL 字符串

<!-- prettier-ignore -->
|用例|unsafe|safe|
|---|---|---|
|单线程（std::io::sink）|9.4580 GiB/s|-|
|单线程（In place）|7.8869 GiB/s|-|
|32KB 单文件解码（dry run, read only）|3.6112 GiB/s|-|
|32KB 单文件解码（RW, tmpfs）|1.4933 GiB/s|1.1948 GiB/s|
|10MB 单文件解码（dry run, read only）|6.6144 GiB/s|-|
|10MB 单文件解码（RW, tmpfs）|5.7140 GiB/s|2.1883 GiB/s|
|32KB 文件并行解码（dry run, read only）|25.460 GiB/s|-|
|32KB 文件并行解码（RW, tmpfs）|28.930 GiB/s|25.808 GiB/s|
|4MB 文件并行解码（dry run, read only）|27.133 GiB/s|-|
|4MB 文件并行解码（RW, tmpfs）|21.860 GiB/s|11.954 GiB/s|

```sh
cargo bench --bench single_thread --no-default-features
cargo bench --bench single_file --no-default-features
cargo bench --bench multi_files --no-default-features -F bin
```

## fuzz

```sh
cargo fuzz run fuzz_target_unsafe --no-default-features -- -max_len=131072 -jobs=32
```

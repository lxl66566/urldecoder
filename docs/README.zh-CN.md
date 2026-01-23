# urldecoder

[English](../README.md) | 简体中文

cli 工具，批量解码 URL，也可以作为 rust lib 使用。性能优先。

解码可以缩短字符串长度，增加可读性，非常适合用于博客、文章和文档中。例如：

```diff
- https://github.com/lxl66566/my-college-files/tree/main/%E4%BF%A1%E6%81%AF%E7%A7%91%E5%AD%A6%E4%B8%8E%E5%B7%A5%E7%A8%8B%E5%AD%A6%E9%99%A2/%E5%B5%8C%E5%85%A5%E5%BC%8F%E7%B3%BB%E7%BB%9F
+ https://github.com/lxl66566/my-college-files/tree/main/信息科学与工程学院/嵌入式系统
```

## 使用方法

### 命令行

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

我是这样使用它的：

```sh
urldecoder -e src/.vuepress/.cache -e src/.vuepress/.temp -e src/.vuepress/dist --escape-space 'src/**/*.md' 'src/.vuepress/components/*' 'src/.vuepress/data/*'
```

### rust 库

前往 [docs.rs](https://docs.rs/urldecoder) 查看文档。

features:

- `bin`: 用于编译命令行，rayon 并行解码 + glob 文件匹配
- `verbose-log`: 启用解码过程中的日志输出
- `safe` (default): 如果 url 解码后不是有效的 utf-8 字符串，则跳过解码

其他：

- 单条 URL 不能超过 64KB。

## benchmark

测试环境：Ryzen 7950x, NixOS

字符串/文本内容：90% 的 ASCII 普通文本 + 10% 的 URL 字符串

### 单线程

进行纯内存的 url 解码测试。

`cargo bench --bench single_thread --no-default-features`

Result: **3.2944 GiB/s**

### 并行解码文件

在 tmpfs 上进行多线程文件解码测试。

`cargo bench --bench multi_files --no-default-features -F bin`

- 900KB 文件：**26.370 GiB/s**
- 10MB 文件：**33.432 GiB/s**

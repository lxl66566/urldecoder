# urldecoder

[English](../README.md) | 简体中文

批量解码文件中的 URL，一个用 Rust 写的~~玩具~~项目。

解码可以缩短字符串长度，增加可读性。例如：

```
- https://github.com/lxl66566/my-college-files/tree/main/%E4%BF%A1%E6%81%AF%E7%A7%91%E5%AD%A6%E4%B8%8E%E5%B7%A5%E7%A8%8B%E5%AD%A6%E9%99%A2/%E5%B5%8C%E5%85%A5%E5%BC%8F%E7%B3%BB%E7%BB%9F
+ https://github.com/lxl66566/my-college-files/tree/main/信息科学与工程学院/嵌入式系统
```

## 安装

### 所有平台

从 [Release](https://github.com/lxl66566/urldecoder/releases) 下载二进制文件。

### Windows

除上述方法外，在 windows 上还可通过 [scoop](https://scoop.sh/) 安装：

```sh
scoop install https://raw.githubusercontent.com/lxl66566/urldecoder/main/urldecoder.json
```

### 使用方法

```sh
Usage: urldecoder [OPTIONS] <FILES>...

Arguments:
  <FILES>...  传入文件样式，使用 glob 匹配

Options:
  -d, --dry-run            仅显示结果，不修改文件
  -v, --verbose            显示更多错误与详细信息
  -e, --exclude <EXCLUDE>  排除文件或文件夹
      --escape-space       不将 `%20` 解码为空格
  -h, --help               打印帮助
  -V, --version            打印版本

Examples:
urldecoder test/t.md        # 解码 test/t.md
urldecoder *.md -e my.md    # 解码当前文件夹下所有 `.md` 结尾的文件，除了 `my.md`
urldecoder **/*             # 解码当前文件夹及其子文件夹的所有文件
```

默认情况下将排除 `node_modules` 文件夹。

这是我如何使用它的一个真实例子：

```sh
urldecoder -e src/.vuepress/.cache -e src/.vuepress/.temp -e src/.vuepress/dist --escape-space 'src/**/*.md'
```

用于在提交前解码我的 vuepress 博客文章内容。

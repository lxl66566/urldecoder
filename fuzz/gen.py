import os
import random

CORPUS_DIR = "corpus/fuzz_target_1"
BUF_SIZE = 64 * 1024


def ensure_dir(directory):
    if not os.path.exists(directory):
        os.makedirs(directory)
        print(f"[+] Created directory: {directory}")


def write_seed(filename, data):
    path = os.path.join(CORPUS_DIR, filename)
    with open(path, "wb") as f:
        f.write(data)
    print(f"    Generated: {filename} ({len(data)} bytes)")


def gen_random_url_part():
    """生成一段包含随机转义字符的 URL 路径"""
    chars = b"abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789-_.~"
    res = bytearray()
    for _ in range(random.randint(5, 20)):
        if random.random() < 0.3:
            # 插入转义字符
            res.extend(b"%" + b"%02X" % random.randint(0, 255))
        else:
            res.append(random.choice(chars))
    return res


def main():
    print(f"[!] Generating seed corpus into {CORPUS_DIR}...")
    ensure_dir(CORPUS_DIR)

    # ---------------------------------------------------------
    # 1. 基础教学样本 (Basic Training)
    # ---------------------------------------------------------
    basic_content = b"Here is a link: http://example.com/foo%20bar and another https://test.org/base%21"
    write_seed("seed_basic_01", basic_content)

    # ---------------------------------------------------------
    # 2. 边界跨越测试 - URL 跨越 64KB 边界
    # ---------------------------------------------------------
    # 场景：填充数据直到 64KB 边界前夕，然后开始写一个长 URL
    # 目的：测试 in_url 状态是否能正确在 loop 之间保持
    padding_len = BUF_SIZE - 10
    padding = b"A" * padding_len
    # 这个 URL 从 index 65526 开始，一直延伸到 65550 左右，横跨两个 buffer
    cross_url = b"https://cross.boundary.com/part1/" + b"B" * 50
    write_seed("seed_boundary_cross_url", padding + cross_url)

    # ---------------------------------------------------------
    # 3. 边界截断测试 - 转义字符被切断 (The Split Percent)
    # ---------------------------------------------------------
    # 场景：Buffer 刚好在 65536 字节处结束，而那里是一个 '%'
    # 下一个 Buffer 以 '20' 开头。你的代码必须保留这个 '%' 到下一次循环。
    padding_len = BUF_SIZE - 1
    padding = b"X" * padding_len
    # 65535 = '%', 65536 = '2', 65537 = '0'
    split_percent = padding + b"%20" + b"C" * 100
    # 此时文件总大小略大于 64KB
    write_seed("seed_boundary_split_percent_1", split_percent)

    # 场景：Buffer 结束于 "%2"
    padding_len = BUF_SIZE - 2
    padding = b"Y" * padding_len
    split_percent_2 = padding + b"%20" + b"D" * 100
    write_seed("seed_boundary_split_percent_2", split_percent_2)

    # ---------------------------------------------------------
    # 4. HTTP 前缀截断
    # ---------------------------------------------------------
    # 场景：Buffer 结尾是 "http:", 下一个 Buffer 是 "//..."
    # 你的 check_url_prefix 可能需要处理这种情况
    padding_len = BUF_SIZE - 5
    padding = b"Z" * padding_len
    split_prefix = padding + b"http://example.com/split_prefix"
    write_seed("seed_boundary_split_prefix", split_prefix)

    # ---------------------------------------------------------
    # 5. 大文件压力测试 (128KB+)
    # ---------------------------------------------------------
    # 包含大量的 URL，确保填满多个缓冲区
    large_data = bytearray()
    while len(large_data) < 135000:  # 约 132KB
        chunk = b" text " + b"http://stress.test/" + gen_random_url_part()
        large_data.extend(chunk)
    write_seed("seed_large_stress", large_data)

    # ---------------------------------------------------------
    # 6. 纯恶意输入 (Fuzzer 喜欢这类)
    # ---------------------------------------------------------
    # 只有 %, 或者是无效的 hex
    malformed = b"http://bad.com/%%%" + b"%GG%00" * 100
    write_seed("seed_malformed", malformed)

    print("[✔] Done. You can now run cargo fuzz.")


if __name__ == "__main__":
    main()

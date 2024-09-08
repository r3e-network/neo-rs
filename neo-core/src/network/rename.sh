#!/bin/bash

# 函数：将驼峰命名转换为蛇形命名
camel_to_snake() {
    echo "$1" | sed -r 's/([a-z0-9])([A-Z])/\1_\2/g' | tr '[:upper:]' '[:lower:]'
}

# 遍历当前目录中的所有 .cs 文件
for file in *.cs; do
    # 检查文件是否存在（避免 *.cs 不匹配任何文件的情况）
    [ -f "$file" ] || continue
    
    # 获取不带扩展名的文件名
    filename="${file%.cs}"
    
    # 转换为蛇形命名
    new_filename=$(camel_to_snake "$filename")
    
    # 重命名文件
    mv "$file" "${new_filename}.rs"
    echo "重命名: $file -> ${new_filename}.rs"
done

echo "重命名完成。"

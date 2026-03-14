#!/usr/bin/env python3
"""
地形点云数据压缩脚本
使用残差网格化 + Base62 编码压缩 3D 地形数据 (x,y,z)，其中 y 是高度

RESOLUTION 参数控制网格精细度（单位：米）
压缩策略：
  1. 创建粗粒度网格存储基础高度
  2. 仅存储显著残差（超过阈值的点）
  3. 使用 Base62 编码减少文本量
"""

import csv
import math
import json
from collections import defaultdict

# ============== 配置参数 ==============
RESOLUTION = 20.0  # 网格分辨率（米），越小越精细但数据量越大
RESIDUAL_THRESHOLD = 0.5  # 残差阈值（米），小于此值的残差不存储（认为地形平坦）
HEIGHT_PRECISION = 0.2  # 高度精度（米）
INPUT_FILE = "sawyer_dted_scan_raw.csv"
OUTPUT_FILE = "terrain_compressed.dat"
META_FILE = "terrain_meta.json"
LUA_OUTPUT = "terrain_data.lua"  # 嵌入数据的 Lua 文件
EMBED_DATA = True  # 是否将数据嵌入 Lua 文件

# Base62 字符集
BASE62_CHARS = "0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz"

def base62_encode(num):
    """将整数编码为 Base62 字符串"""
    if num == 0:
        return BASE62_CHARS[0]
    
    result = []
    negative = num < 0
    num = abs(num)
    
    while num > 0:
        result.append(BASE62_CHARS[num % 62])
        num //= 62
    
    if negative:
        result.append('-')
    
    return ''.join(reversed(result))

def base62_decode(s):
    """将 Base62 字符串解码为整数"""
    if not s:
        return 0
    
    negative = s[0] == '-'
    if negative:
        s = s[1:]
    
    result = 0
    for char in s:
        result = result * 62 + BASE62_CHARS.index(char)
    
    return -result if negative else result

def load_pointcloud(filepath):
    """加载点云数据"""
    points = []
    with open(filepath, 'r') as f:
        reader = csv.reader(f)
        for row in reader:
            if len(row) >= 3:
                x, y, z = float(row[0]), float(row[1]), float(row[2])
                points.append((x, y, z))
    return points

def create_grid(points, resolution):
    """
    创建网格并计算每个网格单元的高度值
    对于每个 (x,z) 网格位置，存储该位置所有点的平均高度
    """
    # 找到边界
    min_x = min(p[0] for p in points)
    max_x = max(p[0] for p in points)
    min_z = min(p[2] for p in points)
    max_z = max(p[2] for p in points)
    
    # 计算网格尺寸
    grid_width = int(math.ceil((max_x - min_x) / resolution)) + 1
    grid_depth = int(math.ceil((max_z - min_z) / resolution)) + 1
    
    # 初始化网格：grid[gx][gz] = [高度列表]
    grid = defaultdict(list)
    
    for x, y, z in points:
        gx = int((x - min_x) / resolution)
        gz = int((z - min_z) / resolution)
        grid[(gx, gz)].append(y)
    
    # 计算每个网格的平均高度
    grid_avg = {}
    for (gx, gz), heights in grid.items():
        grid_avg[(gx, gz)] = sum(heights) / len(heights)
    
    return grid_avg, min_x, min_z, resolution, grid_width, grid_depth

def compress_data(points, grid_avg, min_x, min_z, resolution, residual_threshold):
    """
    压缩数据：
    1. 存储网格元数据
    2. 存储网格高度（Base62 编码）
    3. 仅存储显著残差数据（Base62 编码）
    """
    # 编码网格高度
    grid_data = []
    for (gx, gz), height in sorted(grid_avg.items()):
        # 高度量化为整数（分米精度）
        height_int = int(round(height * 10))
        grid_data.append(f"{base62_encode(gx)},{base62_encode(gz)},{base62_encode(height_int + 10000)}")
    
    # 编码残差点（仅存储显著残差）
    residual_data = []
    significant_count = 0
    
    for x, y, z in points:
        gx = int((x - min_x) / resolution)
        gz = int((z - min_z) / resolution)
        base_height = grid_avg.get((gx, gz), 0)
        residual = y - base_height
        
        # 仅存储显著残差
        if abs(residual) >= residual_threshold:
            # x, z 相对于网格原点的偏移（分米精度）
            x_offset = int(round((x - (min_x + gx * resolution)) * 10))
            z_offset = int(round((z - (min_z + gz * resolution)) * 10))
            residual_int = int(round(residual * 10))
            residual_data.append(f"{base62_encode(gx)},{base62_encode(gz)},{base62_encode(x_offset + 1000)},{base62_encode(z_offset + 1000)},{base62_encode(residual_int + 500)}")
            significant_count += 1
    
    return grid_data, residual_data, significant_count

def save_compressed(grid_data, residual_data, min_x, min_z, resolution, grid_width, grid_depth, output_file, meta_file, lua_output=None):
    """保存压缩数据"""
    # 保存元数据
    meta = {
        "min_x": min_x,
        "min_z": min_z,
        "resolution": resolution,
        "grid_width": grid_width,
        "grid_depth": grid_depth,
        "residual_threshold": RESIDUAL_THRESHOLD,
        "version": 1
    }
    with open(meta_file, 'w') as f:
        json.dump(meta, f, indent=2)
    
    # 保存压缩数据
    with open(output_file, 'w') as f:
        f.write("#GRID|")
        for line in grid_data:
            f.write(line + "|")
        f.write("#RESIDUALS|")
        for line in residual_data:
            f.write(line + "|")
    
    # 生成嵌入数据的 Lua 文件
    if lua_output:
        data_content = "#GRID|" + "|".join(grid_data) + "|#RESIDUALS|" + "|".join(residual_data)
        # 转义 Lua 长字符串
        lua_code = f'''-- 地形数据（嵌入版）- 自动生成
-- 使用方法：local TQ=require("terrain_query"); local tq=TQ.new(TD)
local TD=[[
{data_content}
]]
return TD
'''
        with open(lua_output, 'w') as f:
            f.write(lua_code)
        print(f"生成嵌入数据文件：{lua_output} ({len(lua_code)/1024:.1f} KB)")
    
    return len(grid_data), len(residual_data)

def main():
    print(f"加载点云数据：{INPUT_FILE}")
    points = load_pointcloud(INPUT_FILE)
    print(f"加载了 {len(points)} 个点")
    
    print(f"创建网格（分辨率：{RESOLUTION} 米）...")
    grid_avg, min_x, min_z, resolution, grid_width, grid_depth = create_grid(points, RESOLUTION)
    print(f"网格尺寸：{len(grid_avg)} 个单元")
    
    print(f"压缩数据（残差阈值：{RESIDUAL_THRESHOLD} 米）...")
    grid_data, residual_data, significant_count = compress_data(points, grid_avg, min_x, min_z, RESOLUTION, RESIDUAL_THRESHOLD)
    
    print("保存压缩数据...")
    grid_lines, residual_lines = save_compressed(grid_data, residual_data, min_x, min_z, RESOLUTION, grid_width, grid_depth, 
                   OUTPUT_FILE, META_FILE, LUA_OUTPUT if EMBED_DATA else None)
    
    # 计算压缩率
    original_size = sum(len(f"{p[0]},{p[1]},{p[2]}") for p in points)
    compressed_size = sum(len(line) for line in grid_data) + sum(len(line) for line in residual_data)
    
    print(f"\n压缩统计：")
    print(f"  原始数据：{len(points)} 行 ({original_size / 1024:.1f} KB)")
    print(f"  网格数据：{grid_lines} 行")
    print(f"  残差数据：{residual_lines} 行 (显著残差点：{significant_count} / {len(points)}, {significant_count/len(points)*100:.1f}%)")
    print(f"  压缩后总行数：{grid_lines + residual_lines}")
    print(f"  压缩后大小：{compressed_size / 1024:.1f} KB")
    print(f"  压缩率：{compressed_size/original_size*100:.1f}%")
    print(f"\n输出文件：{OUTPUT_FILE}, {META_FILE}")

if __name__ == "__main__":
    main()

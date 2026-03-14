-- Stormworks 版本测试脚本
-- 模拟 property.getText 环境

-- 模拟 property 对象 (Stormworks API)
property = {}
property._data = nil
property._meta = nil

property.getText = function(key)
    if key == "terrain_data" then return property._data end
    if key == "terrain_meta" then return property._meta end
    return nil
end

-- 加载地形数据文件
local f = io.open("terrain_compressed.dat", "r")
if f then
    local content = f:read("*all")
    f:close()

    property._data = content
    
    print("Loaded terrain_data.lua:", #content, "bytes")
end

-- 加载元数据
f = io.open("terrain_meta.json", "r")
if f then
    property._meta = f:read("*all")
    f:close()
    print("Loaded terrain_meta.json")
end

-- 解析 JSON 辅助函数（测试用）
function parse_json(str)
    local m = {}
    for k, v in str:gmatch([["(min_%w+)"%s*:%s*(%-?%d+%.?%d*)]]) do m[k] = tonumber(v) end
    for k, v in str:gmatch([["(resolution)"%s*:%s*(%-?%d+%.?%d*)]]) do m[k] = tonumber(v) end
    for k, v in str:gmatch([["(grid_%w+)"%s*:%s*(%-?%d+%.?%d*)]]) do m[k] = tonumber(v) end
    for k, v in str:gmatch([["(grid_depth)"%s*:%s*(%d+)]]) do m[k] = tonumber(v) end
    return m
end

-- 加载地形查询模块
dofile("terrain_query_sw.lua")

-- 测试查询
print("\n=== Terrain Query Test ===")

-- 计算边界用于测试
local meta = parse_json(property._meta)
local max_x = meta.min_x + meta.grid_width * meta.resolution
local max_z = meta.min_z + meta.grid_depth * meta.resolution

print(string.format("Coverage: [%.2f, %.2f] x [%.2f, %.2f]", 
    meta.min_x, max_x, meta.min_z, max_z))

local tests = {
    {209, -7770},   -- 范围内
    {-1171.78, -1966.75},   -- 范围内
    {-1153.14, -1935.99},   -- 范围内
    {-1100.00, -2000.00},   -- 范围内
    {-50000, -50000},       -- 范围外
    {50000, 50000},         -- 范围外
}

print("\nSingle queries:")
for _, t in ipairs(tests) do
    local y = terrain_h(t[1], t[2])
    local mark = (y == -1) and " [OUT]" or ""
    print(string.format("  (%.2f, %.2f) -> y=%.2f%s", t[1], t[2], y, mark))
end

-- 批量查询
print("\nBatch query:")
local ys = terrain_hb(tests)
for i, y in ipairs(ys) do
    local mark = (y == -1) and " [OUT]" or ""
    print(string.format("  Point %d: y=%.2f%s", i, y, mark))
end

-- 性能测试
print("\nPerformance test (1000 queries)...")
local start = os.clock()
for i = 1, 1000 do
    local x = meta.min_x + math.random() * (max_x - meta.min_x)
    local z = meta.min_z + math.random() * (max_z - meta.min_z)
    terrain_h(x, z)
end
local elapsed = os.clock() - start
print(string.format("  Time: %.4f sec (%.0f q/s)", elapsed, 1000/elapsed))

-- 清缓存测试
print("\nClear cache...")
terrain_clear_cache()
print("  Done")

print("\n=== Test Complete ===")

#!/usr/bin/env lua
--[[
地形查询性能测试
]]

local TerrainQuery = require("terrain_query")

print("加载地形数据...")
local tq = TerrainQuery.new("terrain_compressed.dat", "terrain_meta.json")

-- 测试点
local test_points = {}
math.randomseed(os.time())
for i = 1, 1000 do
    test_points[i] = {
        x = tq.x0 + math.random() * (tq.res * 100),
        z = tq.z0 + math.random() * (tq.res * 100)
    }
end

print(string.format("测试 %d 次查询...", #test_points))

-- 单次查询测试
local start = os.clock()
for i, p in ipairs(test_points) do
    tq:h(p.x, p.z)
end
local elapsed = os.clock() - start

print(string.format("\n=== 性能测试结果 ==="))
print(string.format("总查询次数：%d", #test_points))
print(string.format("总耗时：%.4f 秒", elapsed))
print(string.format("平均每次：%.4f ms", elapsed / #test_points * 1000))
print(string.format("查询速度：%.0f 次/秒", #test_points / elapsed))

-- 缓存统计
local stats = tq:cs()
if stats.enabled then
    print(string.format("\n=== 缓存统计 ==="))
    print(string.format("缓存命中：%d", stats.hits))
    print(string.format("缓存未命中：%d", stats.misses))
    print(string.format("命中率：%.1f%%", stats.hit_rate))
    print(string.format("缓存大小：%d / 256", stats.size))
end

-- 批量查询测试
print(string.format("\n=== 批量查询测试 ==="))
start = os.clock()
local batch_count = 0
for i = 0, 99 do
    local batch = {}
    for j = 1, 10 do
        local idx = i * 10 + j
        if idx <= #test_points and test_points[idx] and test_points[idx].x then
            table.insert(batch, {test_points[idx].x, test_points[idx].z})
        end
    end
    if #batch > 0 then
        tq:hb(batch)
        batch_count = batch_count + 1
    end
end
elapsed = os.clock() - start
print(string.format("批量查询 %d 批 (%d 次): %.4f 秒", batch_count, batch_count * 10, elapsed))

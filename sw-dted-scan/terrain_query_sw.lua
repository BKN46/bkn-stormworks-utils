-- Terrain Query for Stormworks     
 
CACHE_SIZE = 128
B62 = "0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz"
B62M = {}
for i = 1, 62 do B62M[B62:sub(i, i)] = i - 1 end
 
g_data = nil
g_meta = nil
g_grid = {}
g_residuals = {}
g_cache = {}
g_cache_order = {}
g_init = false
data_str = nil
meta_str = nil
tmp_value = 0
dat_file_num = 925
 
function parse_json(str)
	local m = {}
	for k, v in str:gmatch([["(min_%w+)"%s*:%s*(%-?%d+%.?%d*)]]) do m[k] = tonumber(v) end
	for k, v in str:gmatch([["(resolution)"%s*:%s*(%-?%d+%.?%d*)]]) do m[k] = tonumber(v) end
	for k, v in str:gmatch([["(grid_%w+)"%s*:%s*(%-?%d+%.?%d*)]]) do m[k] = tonumber(v) end
	for k, v in str:gmatch([["(residual_threshold)"%s*:%s*(%-?%d+%.?%d*)]]) do m[k] = tonumber(v) end
	for k, v in str:gmatch([["(version)"%s*:%s*(%d+)]]) do m[k] = tonumber(v) end
	return m
end
 
function b64d(s)
	if not s or s == "" then return 0 end
	local r, neg, i = 0, false, 1
	if s:sub(1, 1) == "-" then neg, i = true, 2 end
	for j = i, #s do
		r = r * 62 + B62M[s:sub(j, j)]
	end
	return neg and -r or r
end
 
function cache_get(k)
	return g_cache[k]
end

function cache_put(k, v)
	local found = false
	for i, key in ipairs(g_cache_order) do
		if key == k then
			found = true
			table.remove(g_cache_order, i)
			break
		end
	end
	
	if #g_cache_order >= CACHE_SIZE then
		local old = table.remove(g_cache_order, 1)
		g_cache[old] = nil
	end
	
	g_cache[k] = v
	table.insert(g_cache_order, k)
end
 
-- Global states for chunked loading
g_load_state = 0
g_prop_idx = 1
g_data_parts = {}
g_line_iter = nil
g_current_sec = nil

function process_init()
	if g_init then return end
	
	-- STATE 0: Read properties in chunks to prevent timeout
	if g_load_state == 0 then
		local chunk_limit = 50 -- Read 50 properties per tick
		for i = 1, chunk_limit do
			if g_prop_idx > dat_file_num then
				g_load_state = 1
				break
			end
			
			local part = property.getText("terrain_data_" .. g_prop_idx)
			if part and part ~= "" then
				table.insert(g_data_parts, part)
				tmp_value = tmp_value + 1
			else
				g_load_state = 1
				break
			end
			g_prop_idx = g_prop_idx + 1
		end
		
	-- STATE 1: Concatenate strings and parse JSON meta
	elseif g_load_state == 1 then
		-- table.concat is drastically faster and memory-safe compared to "str = str .. part"
		data_str = table.concat(g_data_parts) 
		g_data_parts = nil -- Free table memory
		
		meta_str = property.getText("terrain_meta")
		
		if not data_str or not meta_str then
			g_init = true -- Fail gracefully if missing
			return
		end
		
		g_meta = parse_json(meta_str)
		if not g_meta then
			g_init = true
			return
		end
		
		-- Create the iterator for the next state
		g_line_iter = data_str:gmatch("[^|]+")
		g_load_state = 2
		
	-- STATE 2: Parse the database incrementally
	elseif g_load_state == 2 then
		local lines_per_tick = 150 -- Parse 150 database lines per tick
		for i = 1, lines_per_tick do
			local ln = g_line_iter()
			
			if not ln then
				-- We ran out of lines, loading is complete!
				g_init = true
				g_line_iter = nil
				break
			end
			
			if ln == "#GRID" then
				g_current_sec = "g"
			elseif ln == "#RESIDUALS" then
				g_current_sec = "r"
			elseif g_current_sec and ln:find(",") then
				local p = {}
				for x in ln:gmatch("[^,]+") do table.insert(p, x) end
				
				if g_current_sec == "g" and #p >= 3 then
					local gx = b64d(p[1])
					local gz = b64d(p[2])
					local h = (b64d(p[3]) - 10000) / 10
					
					if not g_grid[gx] then g_grid[gx] = {} end
					g_grid[gx][gz] = h
					
				elseif g_current_sec == "r" and #p >= 5 then
					local gx = b64d(p[1])
					local gz = b64d(p[2])
					local xo = (b64d(p[3]) - 1000) / 10
					local zo = (b64d(p[4]) - 1000) / 10
					local re = (b64d(p[5]) - 500) / 10
					
					if not g_residuals[gx] then g_residuals[gx] = {} end
					if not g_residuals[gx][gz] then g_residuals[gx][gz] = {} end
					table.insert(g_residuals[gx][gz], {xo, zo, re})
				end
			end
		end
	end
end
 
function interp(rl, xo, zo, res)
	if not rl or #rl == 0 then return 0 end
	
	if #rl == 1 then
		local r = rl[1]
		local dx, dz = r[1] - xo, r[2] - zo
		if dx*dx + dz*dz < res*res then
			return r[3]
		end
		return 0
	end
	
	local ws, wt, rsq = 0, 0, (res * 1.5) * (res * 1.5)
	
	for _, r in ipairs(rl) do
		local dx, dz = r[1] - xo, r[2] - zo
		local dsq = dx*dx + dz*dz
		
		if dsq < rsq then
			local w = math.exp(-dsq / rsq)
			ws = ws + r[3] * w
			wt = wt + w
		end
	end
	
	return wt > 0 and ws / wt or 0
end
 
function terrain_h(x, z)

	if not g_init or not g_meta then return 0 end	
	--  -1
	local max_x = g_meta.min_x + (g_meta.grid_width or 1000) * g_meta.resolution
	local max_z = g_meta.min_z + (g_meta.grid_depth or 1000) * g_meta.resolution
	
	if x < g_meta.min_x or x > max_x or z < g_meta.min_z or z > max_z then
		return -1
	end
	
	-- 
	local gx = math.floor((x - g_meta.min_x) / g_meta.resolution)
	local gz = math.floor((z - g_meta.min_z) / g_meta.resolution)
	
	-- 
	local ck = gx .. "," .. gz
	local cached = cache_get(ck)
	if cached then return cached end
	
	-- 
	local gx0 = g_meta.min_x + gx * g_meta.resolution
	local gz0 = g_meta.min_z + gz * g_meta.resolution
	local xo, zo = x - gx0, z - gz0
	
	-- 
	local bh = 0
	if g_grid[gx] and g_grid[gx][gz] then
		bh = g_grid[gx][gz]
	else
		-- 
		local sum, cnt = 0, 0
		for dx = -1, 1 do
			for dz = -1, 1 do
				local ng, nz = gx + dx, gz + dz
				if g_grid[ng] and g_grid[ng][nz] then
					sum = sum + g_grid[ng][nz]
					cnt = cnt + 1
				end
			end
		end
		if cnt > 0 then bh = sum / cnt end
	end
	
	-- 
	local re = 0
	if g_residuals[gx] and g_residuals[gx][gz] then
		re = interp(g_residuals[gx][gz], xo, zo, g_meta.resolution)
	else
		-- 
		local ar = {}
		for dx = -1, 1 do
			for dz = -1, 1 do
				if dx == 0 and dz == 0 then goto cont end
				local ng, nz = gx + dx, gz + dz
				if g_residuals[ng] and g_residuals[ng][nz] then
					local ngx = g_meta.min_x + ng * g_meta.resolution
					local ngz = g_meta.min_z + nz * g_meta.resolution
					for _, r in ipairs(g_residuals[ng][nz]) do
						table.insert(ar, {ngx + r[1] - gx0, ngz + r[2] - gz0, r[3]})
					end
				end
				::cont::
			end
		end
		if #ar > 0 then re = interp(ar, xo, zo, g_meta.resolution) end
	end
	
	local res = bh + re
	
	-- 
	cache_put(ck, res)
	
	return res
end
 
function terrain_hb(coords)
	local rs = {}
	for i, c in ipairs(coords) do
		rs[i] = terrain_h(c[1], c[2])
	end
	return rs
end
 
function terrain_clear_cache()
	g_cache = {}
	g_cache_order = {}
end

function onTick()
	-- Drive the state machine until fully loaded
	if not g_init then
		process_init()
		output.setNumber(1, 0) -- Output an altitude of 0 while the system is booting
		return
	end
	
	-- Normal operation once loaded
	x,z=input.getNumber(1),input.getNumber(2)
	output.setNumber(1, terrain_h(x,z))
end

function onDraw()
	screen.drawText(1,1,string.format("g_init: %s", tostring(g_init)))
	
	-- These are initialized as {} at the top, so # is safe, but we'll safeguard them anyway
	screen.drawText(1,6,string.format("g_grid: %d", g_grid and #g_grid or 0))
	screen.drawText(1,11,string.format("g_res: %d", g_residuals and #g_residuals or 0))
	
	-- g_meta starts as nil, so we check if it exists before assigning a 1 or 0
	screen.drawText(1,16,string.format("g_meta: %d", g_meta and 1 or 0))
	
	screen.drawText(1,21,string.format("cache: %d", g_cache_order and #g_cache_order or 0))
	screen.drawText(1,26,string.sub(data_str or "no data", 1, 50))
	screen.drawText(1,31,string.sub(meta_str or "no meta", 1, 50))
	screen.drawText(1,36,string.format("tmp: %d", tmp_value))
end

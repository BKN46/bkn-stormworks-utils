HEARTBEAT_TIMER = 0
-- HEARTBEAT_INTERVAL = property.slider("Heartbeat Interval(Sec)", 1, 600, 1, 5) * 60
HEARTBEAT_INTERVAL = 300
START_TIMER = HEARTBEAT_INTERVAL * 4
LAST_TICK_TIME = 0
-- HTTP_PORT = property.slider("Use HTTP Port", 1000, 9999, 1, 5588)
HTTP_PORT = 5588
TPS_STAT = {}

function onTick(game_ticks)
	-- Regist
	FUNC_MAP = {
		["announce"]=server.announce,
		["save"]=server.save,
	}

	local now_time = server.getTimeMillisec()
	if LAST_TICK_TIME ~= 0 then
		tps = game_ticks / (now_time - LAST_TICK_TIME) * 1000
		table.insert(TPS_STAT, tps)
	end
	LAST_TICK_TIME = now_time

	if START_TIMER > 0 then
		START_TIMER = START_TIMER - 1
		if START_TIMER == 0 then
			server.announce("Watchdog", "No valid backend server found, watchdog will be disabled.")
		end
	end
	if START_TIMER == 0 then return end

	-- heartbeat
	HEARTBEAT_TIMER = HEARTBEAT_TIMER + 1
	if HEARTBEAT_TIMER >= HEARTBEAT_INTERVAL then
		HEARTBEAT_TIMER = 0
		-- calculate average TPS
		local tps_sum = 0
		for i = 1, #TPS_STAT do
			tps_sum = tps_sum + TPS_STAT[i]
		end
		tps = tps_sum / #TPS_STAT
		TPS_STAT = {}
		-- send
		sendToServer("heartbeat", {["tps"] = tps})
	end

	if #TPS_STAT > HEARTBEAT_INTERVAL then
		TPS_STAT = {}
	end
end

function sendToServer(path, data_table)
	local req_path = string.format("/%s", path)
	if data_table ~= nil then
		local data_string = ""
		for key, value in pairs(data_table) do
			data_string = data_string .. key .. "=" .. value .. "&"
		end
		data_string = string.sub(data_string, 1, -2)
		req_path = string.format("/%s?%s", path, data_string)
	end
	-- server.announce("Watchdog req", req_path)
	server.httpGet(HTTP_PORT, req_path)
end

function httpReply(port, request_body, response_body)
	-- server.announce("Watchdog rsp", response_body)
	-- rsp: [function]||[param1]|[param2],...
	local rsp = split(response_body, "||")
	if #rsp < 2 then return end
	START_TIMER = -1
	local func, params = rsp[1], {table.unpack(rsp, 2, #rsp)}

	-- call function
	if FUNC_MAP[func] ~= nil then
		FUNC_MAP[func](table.unpack(params))
	end
end

function startsWith(str, start) return str:sub(1, #start) == start end
function split(a, b) if b == nil then b = "%s" end
	local c = {}
	for d in string.gmatch(a, "([^" .. b .. "]+)") do table.insert(c, d) end
	return c
end
function dump(b) if type(b) == 'table' then local d = '{ '
	for e, f in pairs(b) do if type(e) ~= 'number' then e = '"' ..
				e .. '"'
		end
		d = d .. '[' .. e .. '] = ' .. dump(f) .. ',\n'
	end
	return d .. '} '
else return tostring(b) end
end

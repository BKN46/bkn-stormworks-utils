HEARTBEAT_TIMER = 0
HEARTBEAT_INTERVAL = 5 * 60
LAST_TICK_TIME = server.getTimeMillisec()

function onTick(game_ticks)
	-- calculate TPS
	local now_time = server.getTimeMillisec()
	local tps = game_ticks / (now_time - LAST_TICK_TIME) * 1000
	LAST_TICK_TIME = now_time

	-- heartbeat
	HEARTBEAT_TIMER = HEARTBEAT_TIMER + 1
	if HEARTBEAT_TIMER >= HEARTBEAT_INTERVAL then
		HEARTBEAT_TIMER = 0
		sendToServer("heartbeat", {tps = tps})
	end
end

function sendToServer(path, data_table)
	if data_table ~= nil then
		server.httpGet(5588, string.format("/%s", path))
	else
		local data_string = ""
		for key, value in pairs(data_table) do
			data_string = data_string .. key .. "=" .. value .. "&"
		end
		data_string = string.sub(data_string, 1, -2)
		server.httpGet(5588, string.format("/%s?%s", path, data_string))
	end
end

FUNC_MAP = {
	"announce"=server.announce,
	"save"=server.save,
}

function httpReply(port, request_body, response_body)
	-- rsp: [function]||[param1]|[param2],...
	local rsp = split(response_body, "||")
	if #rsp < 2 then return end
	local func, params = rsp[1], split(rsp[2], "|")

	-- call function
	if FUNC_MAP[func] then
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

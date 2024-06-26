g_savedata = {}


RACE_STARTED, RACE_READY = false, false
RACE_VALID = true
PLAYER_ID, VEHICLE_ID, PASSENGER_ID = -1, -1, -1
PLAYER_NAME, PLAYER_STEAM_ID = "", ""
COST = -1
NOW_POINT = 1
G_TIMER, TIMER = 0, 0

SEND_CACHE = {}
DELAY_EVENTS = {}
USE_HTTP = true

CHECK_POINTS = {
	{-30813, 91253, 8},
	{-29780, 90213, 82},
	{-29715, 89234, 110},
	{-28641, 89312, 81},
	{-28999, 89580, 15},
	{-28436, 89584, 88},
	{-28128, 89798, 28},
	{-28288, 90779, 26},
	{-28600, 90648, 15},
	{-28858, 90968, 3},
	{-29346, 90512, 25},
	{-29682, 90750, 88},
}
HOME_POS = {-31142, 91257, 24}
START_POS = {-31114, 91178, 25}
POINT_SIZE = 10

function onCreate(is_world_create)
	ADDON_INDEX = server.getAddonIndex()
	LOCATION_INDEX, is_success = server.getLocationIndex(ADDON_INDEX, "GARAGE")
	LOCATION_DATA, is_success = server.getLocationData(ADDON_INDEX, LOCATION_INDEX)
end

function onTick(game_ticks)
	if UI_TOP == nil then
		UI_TOP = server.getMapID()
	end
	S_NOTIFY, S_SET_SEAT = server.notify, server.setSeated
	PLAYER_OBJ_ID, is_success = server.getPlayerCharacterID(PLAYER_ID)
	G_TIMER = G_TIMER + 1

	if G_TIMER % 30 == 0 then
		gameSet()
		-- refreshUI(PLAYER_ID)
		if RACE_STARTED then
			vehicle_id, is_success = server.getCharacterVehicle(PLAYER_OBJ_ID)
			if is_success then
				VEHICLE_ID = vehicle_id
			end
			transform_matrix, is_success = server.getVehiclePos(VEHICLE_ID)
			x, z, y = matrix.position(transform_matrix)
			appendData({x, y, z, TIMER, '', ''})
		else
			NOW_POINT = 1
			TIMER = 0
		end
	end
	if G_TIMER % 300 == 0 then
		if RACE_STARTED then
			sendData()
		end
	end

	if RACE_STARTED then
		if server.getObjectData(PASSENGER_ID).dead then
			server.notify(PLAYER_ID, "Failed", "Your assenger dead, please try again.", 2)
			return
		end
		TIMER = TIMER + game_ticks
		checkPoint()
	elseif RACE_READY then
		transform_matrix, is_success = server.getVehiclePos(VEHICLE_ID)
		x, z, y = matrix.position(transform_matrix)
		if dist3(x-CHECK_POINTS[1][1], y-CHECK_POINTS[1][2], z-CHECK_POINTS[1][3])<=POINT_SIZE then
			RACE_STARTED = true
			NOW_POINT = 2
			sendStart()
			server.announce("START", "Race start!")
		end
	end

	for k, v in pairs(DELAY_EVENTS) do
        if v.TIME > 0 then
            DELAY_EVENTS[k].TIME = DELAY_EVENTS[k].TIME - game_ticks
        else
            v.DO(table.unpack(v.PARAM))
            DELAY_EVENTS[k] = nil
        end
    end
end

function onPlayerJoin(steam_id, name, peer_id, admin, auth)
	PLAYER_ID = peer_id
	PLAYER_NAME = name
	PLAYER_STEAM_ID = steam_id
	server.notify(peer_id, "Welcome", string.format("Time Attack Addon loaded\n%s(%s)", PLAYER_NAME, PLAYER_STEAM_ID), 4)
	createUI(peer_id)
end

function onPlayerLeave(steam_id, name, peer_id, admin, auth)

end

function onCustomCommand(full_message, user_peer_id, is_admin, is_auth, command, one, two, three, four, five)

	if (command == "?hello") then
		server.announce("[Server]", "world")
	elseif (command == "?pos") then
		transform_matrix, is_success = server.getPlayerPos(user_peer_id)
		x, y, z = matrix.position(transform_matrix)
		server.notify(user_peer_id, "debug", string.format("(%.2f, %.2f, %.2f)", x, y, z), 2)
	elseif (command == "?draw") then
		createUI(user_peer_id)
	elseif (command == "?home") then
		transform_matrix = matrix.translation(HOME_POS[1], HOME_POS[3], HOME_POS[2])
		is_success = server.setPlayerPos(user_peer_id, transform_matrix)
		RACE_STARTED, RACE_READY = false, false
	elseif (command == "?start") then
		if RACE_READY then
			server.notify(user_peer_id, "Re-ready", "All status reset.", 8)
		end
		RACE_STARTED, TIMER = false, 0
		NOW_POINT = 1
		getReady(user_peer_id, one)
	elseif (command == "?ping") then
		sendPing()
	elseif (command == "?clean") then
		server.cleanVehicles()
		RACE_STARTED, RACE_READY = false, false
	elseif (command == "?switch_upload") then
		if USE_HTTP then
			USE_HTTP = false
			server.notify(user_peer_id, "HTTP", "Auto upload disabled", 8)
		else
			USE_HTTP = true
			server.notify(user_peer_id, "HTTP", "Auto upload enabled", 8)
		end
	elseif (command == "?debug") then
		VEHICLE_DATA, is_success = server.getVehicleComponents(VEHICLE_ID)
		transform_matrix, is_success = server.getVehiclePos(VEHICLE_ID)
		server.announce("DEBUG", dump({
			-- transform_matrix         
			server.getObjectData(PLAYER_OBJ_ID),
			server.getObjectData(PASSENGER_ID)
			-- DELAY_EVENTS
        }), user_peer_id)
	end

end

function onVehicleTeleport(vehicle_id, peer_id, x, y, z)
	if peer_id == -1 then
		return
	end
	RACE_STARTED = false
end

function onVehicleSpawn(vehicle_id, peer_id, x, y, z, cost)
	if RACE_READY or RACE_STARTED then
		RACE_READY, RACE_STARTED = false, false
	end
	if peer_id == -1 then
		return
	end
	VEHICLE_ID = vehicle_id
	COST = cost
	server.notify(peer_id, "Vehicle Spawned", "Enter ?start to start the race", 4)
end

function onVehicleDespawn(vehicle_id, peer_id)
	RACE_READY, RACE_STARTED = false, false
end

function checkPoint()
	transform_matrix, is_success = server.getVehiclePos(VEHICLE_ID)
	x, z, y = matrix.position(transform_matrix)
	transform_matrix, is_success = server.getObjectPos(PASSENGER_ID)
	px, pz, py = matrix.position(transform_matrix)
	transform_matrix, is_success = server.getPlayerPos(PLAYER_ID)
	cx, cz, cy = matrix.position(transform_matrix)

	if dist3(x-px, y-py, z-pz)>20 then
		RACE_STARTED, RACE_READY = false, false
		server.notify(PLAYER_ID, "No Passenger", string.format("Passenger not with the vehicle.\nPlease restart."), 2)
	end

	if dist3(x-CHECK_POINTS[NOW_POINT][1], y-CHECK_POINTS[NOW_POINT][2], z-CHECK_POINTS[NOW_POINT][3])<=POINT_SIZE then
		if NOW_POINT==1 then
			RACE_STARTED, RACE_READY = false, false
			server.notify(PLAYER_ID, "Finish", string.format("Use time %s", getTimeStr(TIMER)), 4)
			server.announce("FINISH", string.format("Use time %s", getTimeStr(TIMER)))
			sendData()
			sendEnd()
		else
			server.notify(PLAYER_ID, "Checkpoint", string.format("You have reached point %d/13\nUse time %s", NOW_POINT, getTimeStr(TIMER)), 4)
			server.announce("CHECKPOINT", string.format("You have reached point %d/13\nUse time %s", NOW_POINT, getTimeStr(TIMER)))
			NOW_POINT = NOW_POINT + 1
			if NOW_POINT > #CHECK_POINTS then
				NOW_POINT = 1
			end
			appendData({x, y, z, TIMER, '', string.format("Point #%d", NOW_POINT)})
		end
	end
end

function getTimeStr(time)
	return string.format("%02d:%02d:%03d", math.floor(time/3600), math.floor(time/60%60), math.floor((time%60)/60*1000))
end

function getReady(user_peer_id, one)
	if one==nil then
		lift = 3
	else
		lift = tonumber(one)
	end
	if VEHICLE_ID == -1 then
		server.notify(user_peer_id, "Vehicle", "Please spawn a vehicle first", 2)
		return
	end
	-- is_success = server.resetVehicleState(VEHICLE_ID)
	VEHICLE_DATA, is_success = server.getVehicleComponents(VEHICLE_ID)
	if #VEHICLE_DATA.components.seats<2 then
		server.notify(peer_id, "Vehicle Invalid", "Vehicle must have at least 2 seats", 2)
		server.removeVehicle(VEHICLE_ID)
		VEHICLE_ID = -1
	else
		getOnVehicle(VEHICLE_ID, user_peer_id, 1, 0)
		transform_matrix = matrix.translation(START_POS[1], START_POS[3] + lift, START_POS[2])
		-- out_matrix = matrix.rotationY(3.14)
		is_success = server.moveVehicle(VEHICLE_ID, transform_matrix)
		-- is_success = server.setVehiclePos(VEHICLE_ID, matrix.multiply(transform_matrix, out_matrix))

		server.killCharacter(PASSENGER_ID)
		PASSENGER_ID, is_success = server.spawnCharacter(
			matrix.translation(START_POS[1] + 15, START_POS[3], START_POS[2] + 10),
			(OUTFIT_TYPE)
		)
		-- server.announce("DEBUG", dump(VEHICLE_DATA))
		seat_name = VEHICLE_DATA.components.seats[2].name
		addDelay(120, S_SET_SEAT, {PASSENGER_ID, VEHICLE_ID, seat_name})
		RACE_READY = true
	end
end


function createUI(peer_id)
	ui_id = server.getMapID()
	server.removeMapID(peer_id, ui_id)
	server.removeMapObject(peer_id, ui_id)
	for key, value in pairs(CHECK_POINTS) do
		if key==1 then
			lastv = CHECK_POINTS[#CHECK_POINTS]
			lastm = matrix.translation(lastv[1],0,lastv[2])
		else
			lastm = tmpm
		end
		tmpm = matrix.translation(value[1],value[3],value[2])
		-- object_id, is_success = server.spawnObject(matrix.translation(value[1],value[3] + 10,value[2]), 57)  
		server.addMapLine(peer_id, ui_id, lastm, tmpm, 1, 70, 70, 70, 255)
		server.addMapObject(peer_id, ui_id, 0, 3, value[1], value[2], 0, 0, 0, 0, string.format("Point #%d", key),
			POINT_SIZE, "Please fly by")
	end
end


function refreshUI(peer_id)
	server.removePopup(peer_id, UI_TOP)
	if RACE_STARTED then
		if NOW_POINT==1 then
			show_progess = #CHECK_POINTS
		else
			show_progess = NOW_POINT - 1
		end
		server.setPopupScreen(peer_id, UI_TOP, "Race", true, string.format("\n\n\n\n\nCheckpoint %d/13\nUse time %s", show_progess, getTimeStr(TIMER)), 0, 1)
	elseif RACE_READY then
		server.setPopupScreen(peer_id, UI_TOP, "Race", true, string.format("\n\n\n\n\nReady now\nGet to start point"), 0, 1)
	else
		server.setPopupScreen(peer_id, UI_TOP, "Race", true, string.format("\n\n\n\n\nYou are not ready yet"), 0, 1)
	end
end


function gameSet()
	server.setGameSetting("vehicle_damage", true)
	server.setGameSetting("vehicle_spawning", false)
	-- server.setGameSetting("third_person_vehicle", false)
	server.setGameSetting("player_damage", true)
	server.setGameSetting("unlock_all_components", true)
	server.setGameSetting("auto_refuel", true)
	server.setGameSetting("fast_travel", true)
	server.setGameSetting("infinite_money", true)
	server.setGameSetting("teleport_vehicle", true)
	server.setGameSetting("infinite_batteries", false)
	server.setGameSetting("infinite_fuel", false)
	server.setGameSetting("engine_overheating", true)
	server.setGameSetting("no_clip", true)
	server.setGameSetting("map_teleport", false)
	server.setGameSetting("photo_mode", true)
	server.setGameSetting("map_show_players", false)
	server.setGameSetting("map_show_vehicles", false)
	server.setGameSetting("show_3d_waypoints", false)
	server.setTutorial(false)
	server.setWeather(0, 0, 0)
end

function addDelay(time, do_func, param)
	table.insert(DELAY_EVENTS, {
		["TIME"] = time,
		["DO"] = do_func,
		["PARAM"] = param,
	})
end

function getOnVehicle(vehicle_id, peer_id, seat_index, retry_times)
	VEHICLE_DATA, is_success = server.getVehicleComponents(vehicle_id)
	-- server.announce("DEBUG", dump(VEHICLE_DATA))
	if not is_success or VEHICLE_DATA==nil or VEHICLE_DATA.components==nil or VEHICLE_DATA.components.seats==nil or VEHICLE_DATA.components.seats[seat_index]==nil then
		if retry_times>=5 then
			server.notify(peer_id, "Failed", "Failed to start, maybe seat not enough or range too long", 2)
			return
		end
		addDelay(180, getOnVehicle, {vehicle_id, peer_id, seat_index, retry_times+1})
		return
	end
	seat_name = VEHICLE_DATA.components.seats[seat_index].name
	-- server.setVehicleSeat(vehicle_id, seat_name, 0, 0, 0, 0, true, false, false, false, false, false)
	object_id, is_success = server.getPlayerCharacterID(peer_id)
	server.setSeated(object_id, vehicle_id, seat_name)
end

function appendData(data)
	table.insert(SEND_CACHE, data)
end

function sendPing()
	sendToServer("ping")
end

function sendStart()
	sendToServer("start", string.format("player=%s&steam_id=%s&cost=%d", PLAYER_NAME, tostring(PLAYER_STEAM_ID), COST))
	SEND_CACHE = {}
end

function sendData()
	sendCacheStrTable = {}
	for k1, v1 in pairs(SEND_CACHE) do
		tmp_line = {}
		for k2, v2 in pairs(v1) do
			table.insert(tmp_line, dumpJson(v2))
		end
		table.insert(sendCacheStrTable, table.concat(tmp_line, ","))
	end
	data = table.concat(sendCacheStrTable, "|||")
	sendToServer("send", string.format("value=%s", data))
	SEND_CACHE = {}
end

function sendEnd()
	sendToServer("end", string.format("time=%d", TIMER))
end

function sendToServer(path, data)
	if not USE_HTTP then
		return
	end
	if data==nil then
		server.httpGet(5588, string.format("/%s", path))
	else
		server.httpGet(5588, string.format("/%s?%s", path, data))
	end
end

function httpReply(port, request_body, response_body)
	if response_body == "ping:yeah" then
		S_NOTIFY(PLAYER_ID, "Ping", "Upload works!", 4)
	elseif response_body == "ping:nah" then
		S_NOTIFY(PLAYER_ID, "Ping", "Upload failed!", 4)
	elseif response_body == "upload:done" then
		S_NOTIFY(PLAYER_ID, "Race data", "Record successfully uploaded", 4)
	end
end

function startsWith(str, start) return str:sub(1, #start) == start end
function dist2(x, y) return math.sqrt(x * x + y * y) end
function dist3(x, y, z) return (x * x + y * y + z * z)^(1/3) end
function split(a, b) if b == nil then b = "%s" end
	local c = {}
	for d in string.gmatch(a, "([^" .. b .. "]+)") do table.insert(c
			, d)
	end
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
function dumpJson(b)if type(b)=='table'then local d='{'for e,f in pairs(b)do if type(e)~='number'then e='"'..e..'"'end;d=d..'"'..e..'":'..dumpJson(f)..','end;return d..'}'else return tostring(b) end end

g_savedata = {}


RACE_STARTED = false
RACE_VALID = true
PLAYER_ID, VEHICLE_ID, PASSENGER_ID = -1, -1, -1
COST = -1
NOW_POINT = 1
TIMER = 0
DELAY_EVENTS = {}

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
START_POS = {-31114, 91178, 25}
POINT_SIZE = 10

function onCreate(is_world_create)
	ADDON_INDEX = server.getAddonIndex()
	LOCATION_INDEX, is_success = server.getLocationIndex(ADDON_INDEX, "GARAGE")
	LOCATION_DATA, is_success = server.getLocationData(ADDON_INDEX, LOCATION_INDEX)
end

function onTick(game_ticks)
	if game_ticks % 30 == 0 then
		-- gameSet()
		if RACE_STARTED then
			object_id, is_success = server.getPlayerCharacterID(PLAYER_ID)
			vehicle_id, is_success = server.getCharacterVehicle(object_id)
			if is_success then
				VEHICLE_ID = vehicle_id
			end
		else
			NOW_POINT = 1
			-- if PASSENGER_ID ~= -1 then
			-- 	server.killCharacter(PASSENGER_ID)
			-- 	PASSENGER_ID = -1
			-- end
		end
	end

end

function onPlayerJoin(steam_id, name, peer_id, admin, auth)
	PLAYER_ID = peer_id
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
	elseif (command == "?start") then
		if VEHICLE_ID == -1 then
			server.notify(user_peer_id, "Vehicle", "Please spawn a vehicle first", 2)
			return
		end
		VEHICLE_DATA, is_success = server.getVehicleComponents(VEHICLE_ID)
		if #VEHICLE_DATA.components.seats<2 then
			server.notify(peer_id, "Vehicle Invalid", "Vehicle must have at least 2 seats", 2)
			server.removeVehicle(VEHICLE_ID)
			VEHICLE_ID = -1
			return
		end
		getOnVehicle(VEHICLE_ID, user_peer_id, 1, 0)
		transform_matrix = matrix.translation(START_POS[1], START_POS[3] + 5, START_POS[2])
		is_success = server.moveVehicle(VEHICLE_ID, transform_matrix)


		PASSENGER_ID, is_success = server.spawnCharacter(
			matrix.translation(START_POS[1] + 10, START_POS[3], START_POS[2] + 10),
			(OUTFIT_TYPE)
		)
		-- server.announce("DEBUG", dump(VEHICLE_DATA))
		seat_name = VEHICLE_DATA.components.seats[2][1]
		-- server.setSeated(PASSENGER_ID, VEHICLE_ID, seat_name)
	end

end


function onVehicleTeleport(vehicle_id, peer_id, x, y, z)
	if peer_id == -1 then
		return
	end
	RACE_STARTED = false
end

function onVehicleSpawn(vehicle_id, peer_id, x, y, z, cost)
	if peer_id == -1 then
		return
	end
	VEHICLE_ID = vehicle_id
	COST = cost
	server.notify(peer_id, "Vehicle Spawned", "Enter ?start to start the race", 2)
end

function createUI(peer_id)
	ui_id = server.getMapID()
	server.removeMapID(peer_id, ui_id)
	for key, value in pairs(CHECK_POINTS) do
		if key==1 then
			lastv = CHECK_POINTS[#CHECK_POINTS]
			lastm = matrix.translation(lastv[1],0,lastv[2])
		else
			lastm = tmpm
		end
		tmpm = matrix.translation(value[1],value[3],value[2])
		server.addMapLine(peer_id, ui_id, lastm, tmpm, 1, 70, 70, 70, 255)
		server.addMapObject(peer_id, ui_id, 0, 9, value[1], value[2], 0, 0, 0, 0, string.format("Point #%d", key),
			POINT_SIZE, "Please fly by")
	end
end

function gameSet()
	server.setGameSetting("vehicle_damage", true)
	server.setGameSetting("vehicle_spawning", false)
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

function getOnVehicle(vehicle_id, peer_id, seat_index, retry_times)
	VEHICLE_DATA, is_success = server.getVehicleComponents(vehicle_id)
	-- server.announce("DEBUG", dump(VEHICLE_DATA))
	if not is_success or VEHICLE_DATA==nil or VEHICLE_DATA.components==nil or VEHICLE_DATA.components.seats==nil or VEHICLE_DATA.components.seats[seat_index]==nil then
		if retry_times>=5 then
			server.notify(peer_id, "Failed", "Failed to start, maybe seat not enough or range too long", 2)
			return
		end
		addDelay(120, getOnVehicle, {vehicle_id, peer_id, seat_index, retry_times+1})
		return
	end
	seat_name = VEHICLE_DATA.components.seats[seat_index][1]
	-- server.setVehicleSeat(vehicle_id, seat_name, 0, 0, 0, 0, true, false, false, false, false, false)
	object_id, is_success = server.getPlayerCharacterID(peer_id)
	server.setSeated(object_id, vehicle_id, seat_name)
end

function httpReply(port, request_body, response_body) end
function startsWith(str, start) return str:sub(1, #start) == start end
function dist2(x, y) return math.sqrt(x * x + y * y) end
function dist3(x, y, z) return math.pow(x * x + y * y + z * z, 1/3) end

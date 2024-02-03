g_savedata = {}


RACE_STARTED = false
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
		else
			NOW_POINT = 1
		end
	end

end

function onPlayerJoin(steam_id, name, peer_id, admin, auth)
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
		server.notify(user_peer_id, string.format("(%.2f, %.2f, %.2f)", x, y, z))
	elseif (command == "?draw") then
		createUI(user_peer_id)
	end

end


function onVehicleTeleport(vehicle_id, peer_id, x, y, z)
    if peer_id == -1 then
        return
    end
	RACE_STARTED = false
    -- server.announce("DEBUG", dump({vehicle_id, peer_id}))
    -- for team_index = 1, #SAVEDATA.TEAM_VEHICLES, 1 do
    --     team = SAVEDATA.TEAM_VEHICLES[team_index]
    --     if team[vehicle_id] ~= nil then

    --     end
    -- end
    -- server.setWeather(50,50,50)
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

function startsWith(str, start) return str:sub(1, #start) == start end
function dist2(x, y) return math.sqrt(x * x + y * y) end
function dist3(x, y, z) return math.pow(x * x + y * y + z * z, 1/3) end
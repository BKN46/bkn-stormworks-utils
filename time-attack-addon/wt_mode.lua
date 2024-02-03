--- Note, minimizer functionality can be disabled in your project settings. (right click -> Folder Settings)
--- A large scale update for supporting Addon work is in the works, so keep an eye on the extension!

--[[
        ["TEAMS"] = {
            [1] = {
                [#steam_id#] = {
                    ["STATUS"]=#int#,
                    ["RETURN_TIMER"] = 0,
                    ["UITOP"]=#UIID#,
                    ["UIMID"]=#UIID#,
                    ["UITL"]=#UIID#,
                    ["UIBOT"]=#UIID#,
                },
            }
        }
        ["TEAM_VEHICLES"] = {
            [1] = {
                [#vehicle_id#] = {
                    ["HP"] = #int#,
                    ["MAXHP"] = #int#,
                    ["CREATOR"] = #peer_id#,
                    ["COST"] = #cost#,
                },
            }
        },
    ]] --

INIT_MONEY = 150000
INIT_DATA = {
    ["GAME_END"] = false,
    ["TEAMS"] = { {}, {} },
    ["TEAM_VEHICLES"] = { {}, {} },
    ["TEAM_BASES"] = {
        { 1321.9, 8, -3766.9 },
        { 4106.5, 10, -5925 },
    },
    ["TEAM_SCORES"] = { 100, 100 },
    ["MONEY_POOL"] = { INIT_MONEY, INIT_MONEY },
    ["WATCH_VEHICLES"] = {},
    ["POINT_CAP"] = {
        { 1486.8, 8, -5662.3, 0, 0 },
        { 4135.7, 10, -3872.6, 0, 0 },
    },
    ["FLAGS"] = {},
    ["FLAG_POINT"] = {
        { 2702, 13, -4936 },
    },
    ["BORDER"] = {
        {1500, -3125},
        {2500, -3000},
        {3500, -2750},
        {5250, -3000},
        {6250, -3750},
        {6032, -4870},
        {5820, -5820},
        {4620, -6240},
        {4500, -6640},
        {3800, -6700},
        {2300, -7200},
        {1150, -5930},
        {480, -5480},
        {-930, -5180},
        {-260, -4300},
        {370, -4070},
        {840, -3500},
    },
}
BASE_WARN_ZONE, BASE_PROHIBIT_ZONE = 1000, 400
POINT_SIZE = 100
GAME_START = false

DELAY_EVENTS = {}

function addDelay(time, do_func, param)
    table.insert(DELAY_EVENTS, {
        ["TIME"] = time,
        ["DO"] = do_func,
        ["PARAM"] = param,
    })
end

DETECT_INTERVAL, DETECT_TIMER = 60, 1
SETTLE_INTERVAL, SETTLE_TIMER = 600, 1

function onCreate(is_world_create)
    if is_world_create then
        INIT_MONEY = property.slider("Team money limit", 0, 1000000, 1000, 1500000)
        BASE_WARN_ZONE = property.slider("Spawncamp warning range", 0, 5000, 50, 1000)
        BASE_PROHIBIT_ZONE = property.slider("Spawncamp teleport range", 0, 5000, 50, 400)
    end
    server.setCurrency(INIT_MONEY * 10, 1000)
    server.setGameSetting("vehicle_damage", true)
    server.setGameSetting("vehicle_spawning", false)
    server.setGameSetting("player_damage", true)
    server.setGameSetting("unlock_all_components", true)
    server.setGameSetting("auto_refuel", true)
    server.setGameSetting("fast_travel", false)
    server.setGameSetting("infinite_money", false)
    server.setGameSetting("teleport_vehicle", false)
    server.setGameSetting("infinite_batteries", false)
    server.setGameSetting("infinite_fuel", false)
    server.setGameSetting("engine_overheating", true)
    server.setGameSetting("no_clip", false)
    server.setGameSetting("map_teleport", false)
    server.setGameSetting("photo_mode", false)
    server.setTutorial(false)
    ADDON_INDEX = server.getAddonIndex()
    LOCATION_INDEX, is_success = server.getLocationIndex(ADDON_INDEX, "GARAGE")
    LOCATION_DATA, is_success = server.getLocationData(ADDON_INDEX, LOCATION_INDEX)

end

function onDestroy()
    for vehicle_id, spawn_pos in pairs(SAVEDATA.FLAGS) do
        despawnFlag(vehicle_id)
    end
end

-- SAVEDATA table that persists between game sessions
SAVEDATA = INIT_DATA
g_savedata = INIT_DATA

function onTick(game_ticks)
    onTickFunction(game_ticks)
    -- local status, ret_info = pcall(onTickFunction, game_ticks)
    -- if not status and ret_info then
    --     server.announce("DEBUG", ret_info)
    -- end
end 

function onTickFunction(game_ticks)
    if DETECT_TIMER > DETECT_INTERVAL then
        DETECT_TIMER = 1
        refreshUI()
        -- Main part
        checkMapLimit()
        vehicleEvent()
        watchVehicleEvent()
        flagEvent()
        checkWin()
    else
        DETECT_TIMER = DETECT_TIMER + 1
    end

    if SETTLE_TIMER > SETTLE_INTERVAL then
        SETTLE_TIMER = 1
        -- score settlement
        capEvent()
    else
        SETTLE_TIMER = SETTLE_TIMER + 1
    end

    -- delay event
    for k, v in pairs(DELAY_EVENTS) do
        if v.TIME > 0 then
            DELAY_EVENTS[k].TIME = DELAY_EVENTS[k].TIME - 1
        else
            v.DO(table.unpack(v.PARAM))
            DELAY_EVENTS[k] = nil
        end
    end
end

function onPlayerJoin(steam_id, name, peer_id, admin, auth)
    server.announce("[Server]", name .. " joined the game")
    server.notify(peer_id, "Tips", "Enter ?help for more game mode info.", 8)
    createUI(peer_id)
    object_id = server.getPlayerCharacterID(peer_id)
    server.addAuth(peer_id)
end

function onPlayerLeave(steam_id, name, peer_id, admin, auth)
    server.announce("[Server]", name .. " left the game")
end

function onCustomCommand(full_message, user_peer_id, is_admin, is_auth, command, one, two, three, four, five)

    if (command == "?help") then
        server.notify(user_peer_id, "Tips",
            "[?join_team #team_id#] Join a team\n[?exit_team] Exit a team\n[?team] Show team info\n[?home] Teleport back to spawn after 30sec.\n[?artillery #x# #y# #z#] Call for a artillery strike, cost 1000$.\n[?watch] Get on watcher vehicle\n[?tp #x# #z#] Teleport when you on a watcher vehicle", 8)
        if is_admin then
            server.notify(user_peer_id, "Admin tips",
                "[?reset] Reset game data\n[?rand #1# #2#] Generate a random integer between #1# and #2#\n[?random_weather] Configure random weather\n[?set_money #team_id# #money#] Set team money"
                , 8)
        end
    end

    if (command == "?reset") and is_admin then
        SAVEDATA.TEAMS = { {}, {} }
        SAVEDATA.TEAM_VEHICLES = { {}, {} }
        SAVEDATA.TEAM_SCORES = { 100, 100 }
        SAVEDATA.GAME_END = false
        onDestroy()
        SAVEDATA.FLAGS = {}
        SAVEDATA.MONEY_POOL = { INIT_MONEY, INIT_MONEY }
        SAVEDATA.POINT_CAP = {
            { 1486.8, 8, -5662.3, 0, 0 },
            { 4135.7, 10, -3872.6, 0, 0 },
        }
        GAME_START = false
        server.setGameSetting("vehicle_spawning", false)
        server.cleanVehicles()
        server.clearRadiation()
        server.announce("[Server]", "Game reset!")
    end
    if (command == "?game_start") and is_admin then
        GAME_START = true
        server.setGameSetting("vehicle_spawning", true)
        server.announce("[Game]", "Game started!")
    end
    if (command == "?save") and is_admin then
        g_savedata = SAVEDATA
        server.announce("[Server]", "Game saved!")
    end
    if (command == "?load") and is_admin then
        SAVEDATA = g_savedata
        server.announce("[Server]", "Game loaded!")
    end
    if (command == "?flag") and is_admin then
        spawnFlag(SAVEDATA.FLAG_POINT[1][1], SAVEDATA.FLAG_POINT[1][2], SAVEDATA.FLAG_POINT[1][3])
        server.announce("GAME", "Flag spawned!")
    end
    if (command == "?rand") and is_admin then
        server.announce("[Server]", string.format("%d", math.random(tonumber(one), tonumber(two))))
    end
    if (command == "?set_money") and is_admin then
        team_id, tmp=tonumber(one), tonumber(two)
        SAVEDATA.MONEY_POOL[team_id] = tmp
        server.announce("[Server]", string.format("Team %d money set to %d$", team_id, tmp))
    end
    if (command == "?set_zone_range") and is_admin then
        BASE_WARN_ZONE, BASE_PROHIBIT_ZONE = math.floor(tonumber(one)), math.floor(tonumber(two))
        server.announce("[Server]", string.format("BASE_WARN_ZONE set to %d\nBASE_PROHIBIT_ZONE set to %d$", BASE_WARN_ZONE, BASE_PROHIBIT_ZONE))
    end
    if (command == "?debug") and is_admin then
        --server.notify(user_peer_id, "selfId", user_peer_id, 8)
        server.announce("DEBUG", dump({
            ["TEAMS"]=SAVEDATA.TEAMS,
            ["TEAM_VEHICLES"]=SAVEDATA.TEAM_VEHICLES,
            ["FLAGS"]=SAVEDATA.FLAGS,
            ["CP"]=SAVEDATA.POINT_CAP,
        }), user_peer_id)
        -- server.announce("DEBUG", dump({getPeerIdByCharId(tonumber(one))}))
        -- server.notify(user_peer_id, "debug", dump(server.getPlayerPos(user_peer_id)), 8)
        --server.notify(user_peer_id, "players", dump(server.getPlayers()), 8)
    end
    if (command == "?debugv") and is_admin then
        VEHICLE_DATA, is_success = server.getVehicleData(tonumber(one))
        server.announce("[Server]", dump(VEHICLE_DATA), user_peer_id)
    end

    if (command == "?random_weather") and is_admin then
        fog, rain, wind = math.max(math.random(60)-20,0), math.max(math.random(200)-100,0), math.max(math.random(50)-10,0)
        server.setWeather(fog*0.01,rain*0.01,wind*0.01)
        server.announce("[Server]", string.format("Game weather set to fog:%d, rain:%d, wind:%d!", fog, rain, wind))
    end

    -- player commands
    if (command == "?join_team") then
        steamid = getSteamByPeerId(user_peer_id)
        for team_index = 1, #SAVEDATA.TEAMS, 1 do
            if SAVEDATA.TEAMS[team_index][steamid] ~= nil then
                server.notify(user_peer_id, "Failed", "You already in a team.", 2)
                return
            end
        end
        team_id = tonumber(one)
        SAVEDATA.TEAMS[team_id][steamid] = {
            ["STATUS"] = 1,
            ["RETURN_TIMER"] = 0,
            ["UITOP"] = server.getMapID(),
            ["UIMID"] = server.getMapID(),
            ["UITL"] = server.getMapID(),
            ["UIBOT"] = server.getMapID(),
        }
        --server.notify(user_peer_id, "debug", steamid, 8)
        server.notify(user_peer_id, "Successed", "You successfully join a team.", 4)
        server.announce("[Server]",
            string.format("Player %s joined team %d!", server.getPlayerName(user_peer_id), team_id))
    end
    if (command == "?exit_team") then
        steamid = getSteamByPeerId(user_peer_id)
        for team_index = 1, #SAVEDATA.TEAMS, 1 do
            if SAVEDATA.TEAMS[team_index][steamid] ~= nil then
                SAVEDATA.TEAMS[team_index][steamid] = nil
                server.notify(user_peer_id, "Successed", "You successfully exit a team.", 4)
                server.announce("[Server]",
                    string.format("Player %s exited team %d!", server.getPlayerName(user_peer_id), team_index))
                return
            end
        end
        server.notify(user_peer_id, "Failed", "You are not in a team.", 2)
    end
    if (command == "?team") then
        team_id = getPlayerTeam(user_peer_id)
        player_list = ""
        for team_index = 1, #SAVEDATA.TEAMS, 1 do
            player_list = player_list .. string.format("# Team %d #\n", team_index)
            for steamid, info in pairs(SAVEDATA.TEAMS[team_index]) do
                player_list = player_list .. server.getPlayerName(getPeerIdBySteam(steamid)) .. "\n"
            end
        end
        server.notify(user_peer_id, "Player list", player_list, 8)
        if team_id == 0 then
            -- server.notify(user_peer_id, "Failed", "You are not in a team.", 2)
            return
        end
        server.notify(user_peer_id, "Team info",
            string.format("Your team has %.2f$\n%d players\n%d vehicles now\n%d score", SAVEDATA.MONEY_POOL[team_id],
                tablelength(SAVEDATA.TEAMS[team_id]), tablelength(SAVEDATA.TEAM_VEHICLES[team_id]),
                SAVEDATA.TEAM_SCORES[team_id]), 8)
    end
    if (command == "?home") then
        team_id = getPlayerTeam(user_peer_id)
        if team_id == 0 then
            server.notify(user_peer_id, "Failed", "You are not in a team.", 2)
            return
        end
        self_base = SAVEDATA.TEAM_BASES[team_id]
        server.notify(user_peer_id, "NOTICE", string.format("You'll transport back home in 30sec.", team_mp), 8)
        addDelay(1800, server.setPlayerPos, { user_peer_id, matrix.translation(self_base[1], self_base[2], self_base[3]) })
    end
    if (command == "?artillery") and GAME_START then
        team_id = getPlayerTeam(user_peer_id)
        if team_id == 0 then
            server.notify(user_peer_id, "Failed", "You are not in a team.", 2)
            return
        elseif SAVEDATA.MONEY_POOL[team_id] < 1000 then
            server.notify(user_peer_id, "Failed", "No enough money.", 2)
            return
        end
        x, y, z = tonumber(one), tonumber(two), tonumber(three)
        for base_id, base_pos in pairs(SAVEDATA.TEAM_BASES) do
            if dist2(base_pos[1]-x,base_pos[3]-z) < BASE_WARN_ZONE and base_id~=team_id then
                server.notify(user_peer_id, "Failed", "Too close to enemy base.", 2)
                return
            end
        end
        SAVEDATA.MONEY_POOL[team_id] = SAVEDATA.MONEY_POOL[team_id] - 1000
        server.notify(user_peer_id, "Artillery", string.format("Artillery coming in 10sec.\nTarget (%d, %d, %d)\nCost 1000$ (%.2f left)", x, y, z, SAVEDATA.MONEY_POOL[team_id]), 4)
        addDelayMapObject(18, x, z, "Artillery Strike", 100, "Artillery Strike", 255, 0, 0, 255, 1800)
        addDelay(600, artilleryStrike, { x, y, z, 100, 0.3, 5, 180})
        addDelay(1500, artilleryStrike, { x, y, z, 50, 0.3, 10, 30})
    end
    if (command == "?supply") and GAME_START then
        team_id = getPlayerTeam(user_peer_id)
        if team_id == 0 then
            server.notify(user_peer_id, "Failed", "You are not in a team.", 2)
            return
        elseif SAVEDATA.MONEY_POOL[team_id] < 2500 then
            server.notify(user_peer_id, "Failed", "No enough money.", 2)
            return
        end
        x, z = tonumber(one), tonumber(two)
        for base_id, base_pos in pairs(SAVEDATA.TEAM_BASES) do
            if dist2(base_pos[1]-x,base_pos[3]-z) < BASE_WARN_ZONE and base_id~=team_id then
                server.notify(user_peer_id, "Failed", "Too close to enemy base.", 2)
                return
            end
        end
        SAVEDATA.MONEY_POOL[team_id] = SAVEDATA.MONEY_POOL[team_id] - 2500
        server.notify(user_peer_id, "Supply", string.format("Supply coming in 10sec.\nTarget (%d, %d)\nCost 2500$ (%.2f left)", x, z, SAVEDATA.MONEY_POOL[team_id]), 4)
        addDelayMapObject(2, x, z, "Airdrop Supply", 50, "Airdrop Supply", 0, 255, 0, 255, 1200)
        addDelay(600, spawnAddonVehicle, { 1, x, 300, z})
    end
    if (command == "?clean_my") and GAME_START then
        tmp, tmpm = 0, 0
        team_index = getPlayerTeam(user_peer_id)
        if team_id == 0 then
            server.notify(user_peer_id, "Failed", "You are not in a team.", 2)
            return
        end
        for team_id = 1, #SAVEDATA.TEAM_VEHICLES, 1 do
            team = SAVEDATA.TEAM_VEHICLES[team_id]
            for vehicle_id, vehicle_data in pairs(team) do
                if vehicle_data.CREATOR == user_peer_id then
                    hp_ratio = vehicle_data.HP / vehicle_data.MAXHP
                    recycle_cost = hp_ratio * vehicle_data.COST * 0.5
                    SAVEDATA.MONEY_POOL[team_index] = SAVEDATA.MONEY_POOL[team_index] + recycle_cost
                    server.despawnVehicle(vehicle_id, true)
                    tmp = tmp + 1
                    tmpm = tmpm + recycle_cost
                end
            end
        end
        server.notify(user_peer_id, "Clean", string.format("You cleaned %d vehicles for %.2f$.", tmp, tmpm), 4)
    end
    if (command == "?watch") then
        team_id = getPlayerTeam(user_peer_id)
        if team_id ~= 0 then
            server.notify(user_peer_id, "Failed", "You must not in a team.", 2)
            return
        end
        spawnWatcherHeli(user_peer_id)
    end
    if (command == "?tp") then
        for vehicle_id, peer_id in pairs(SAVEDATA.WATCH_VEHICLES) do
            if peer_id==user_peer_id then
                x, y, z = tonumber(one), tonumber(two), tonumber(three)
                server.notify(user_peer_id, "TP", "Teleport will be in 3sec.", 8)
                spawnWatcherHeli(user_peer_id, x, y, z, 180)
                return
            end
        end
        server.notify(user_peer_id, "Failed", "You must on a watch vehicle.", 2)
        -- transform_matrix = matrix.translation(x, 400, z)
        -- vehicle_id = SAVEDATA.WATCH_VEHICLES[user_peer_id]
        -- is_success = server.setVehiclePosSafe(vehicle_id, transform_matrix)
        -- server.setPlayerPos(user_peer_id, transform_matrix)
        -- getOnVehicle(vehicle_id, user_peer_id, 1)
    end
end

function onVehicleSpawn(vehicle_id, peer_id, x, y, z, cost)
    if peer_id == -1 then
        return
    end

    team_index = getPlayerTeam(peer_id)
    team_mp = SAVEDATA.MONEY_POOL[team_index]
    if team_index == 0 then
        for k, v in pairs(server.getPlayers()) do
            if v.id==peer_id and v.admin then
                return
            end
        end
        server.notify(peer_id, "Failed", string.format("You have to join a team to spawn vehicle.", team_mp), 2)
        server.despawnVehicle(vehicle_id, true)
        return
    elseif GAME_START==false then
        server.notify(peer_id, "Failed", string.format("Game not started yet.", team_mp), 2)
    elseif team_mp < cost then
        server.notify(peer_id, "Failed", string.format("Team don't have enough money. (%.2f$ left)", team_mp), 2)
        server.despawnVehicle(vehicle_id, true)
        return
    else
        SAVEDATA.MONEY_POOL[team_index] = team_mp - cost
        teamNotification(team_index, "Vehicle spawned",
            string.format("%s spawned a vehicle cost %.2f$ (team has %.2f$ left)", server.getPlayerName(peer_id), cost,
                team_mp - cost), 4)
        addDelay(60, registVehicleData, { vehicle_id, peer_id, cost, 0 })
    end
end

function registVehicleData(vehicle_id, peer_id, cost, try_times)
    VEHICLE_DATA, is_success = server.getVehicleData(vehicle_id)
    if not is_success or VEHICLE_DATA==nil or VEHICLE_DATA.voxels ==nil then
        if try_times >=10 then
            server.notify(peer_id, "Failed", "Vehicle regist failed due to server lag.", 8)
            return
        end
        addDelay(120, registVehicleData, { vehicle_id, peer_id, cost, try_times+1 })
        return
    end
    team_id = getPlayerTeam(peer_id)
    if team_id <= 0 then
        return
    end
    tmp = {
        ["HP"] = VEHICLE_DATA.voxels,
        ["MAXHP"] = VEHICLE_DATA.voxels,
        ["CREATOR"] = peer_id,
        ["COST"] = cost,
    }
    SAVEDATA.TEAM_VEHICLES[team_id][vehicle_id] = tmp
    -- server.announce("DEBUG", dump(tmp))
    server.notify(peer_id, "Success", string.format("Vehicle %d registed for team %d", vehicle_id, team_id), 4)

    -- server.announce("[DEBUG]", string.format("spawned vehicle id: %d", vehicle_id))
    -- server.announce("[DEBUG]", dump(VEHICLE_DATA))
end

function onVehicleDamaged(vehicle_id, damage_amount, voxel_x, voxel_y, voxel_z, body_index)
    -- server.announce("[DEBUG]", dump({vehicle_id, damage_amount, body_index}))
    for team_index = 1, #SAVEDATA.TEAM_VEHICLES, 1 do
        team = SAVEDATA.TEAM_VEHICLES[team_index]
        if team[vehicle_id] ~= nil then
            team[vehicle_id].HP = team[vehicle_id].HP - damage_amount
            if team[vehicle_id].HP <= 0 then
                -- server.despawnVehicle(vehicle_id, true)
                -- team[vehicle_id] = nil
                -- SAVEDATA.TEAM_SCORES[team_index] = SAVEDATA.TEAM_SCORES[team_index] - 2
                team[vehicle_id].HP=0
            elseif team[vehicle_id].HP > team[vehicle_id].MAXHP then
                team[vehicle_id].HP = team[vehicle_id].MAXHP
            end
            return
        end
    end
end

function onVehicleTeleport(vehicle_id, peer_id, x, y, z)
    if peer_id == -1 then
        return
    end
    -- server.announce("DEBUG", dump({vehicle_id, peer_id}))
    -- for team_index = 1, #SAVEDATA.TEAM_VEHICLES, 1 do
    --     team = SAVEDATA.TEAM_VEHICLES[team_index]
    --     if team[vehicle_id] ~= nil then

    --     end
    -- end
    -- server.setWeather(50,50,50)
end

function onVehicleDespawn(vehicle_id, peer_id)
    if peer_id == -1 then
        return
    end
    -- server.announce("DEBUG", dump({"despawn", vehicle_id, peer_id}))
    for team_index = 1, #SAVEDATA.TEAM_VEHICLES, 1 do
        team = SAVEDATA.TEAM_VEHICLES[team_index]
        if team[vehicle_id] ~= nil then
            hp_ratio = team[vehicle_id].HP / team[vehicle_id].MAXHP
            recycle_cost = hp_ratio * team[vehicle_id].COST * 0.95
            SAVEDATA.MONEY_POOL[team_index] = SAVEDATA.MONEY_POOL[team_index] + recycle_cost
            server.notify(peer_id, "Recycle",
                string.format("You recycled a vehicle for %.2f$.\n(%.2f HP * %.2f$ * 0.95)", recycle_cost, hp_ratio,
                    team[vehicle_id].COST), 8)
            team[vehicle_id] = nil
            return
        end
    end
end

-- CHECK

function checkMapLimit()
    for team_id = 1, #SAVEDATA.TEAMS, 1 do
        team = SAVEDATA.TEAMS[team_id]
        --server.announce("[Server]", dump(pairs(team)))
        for key, value in pairs(team) do
            peer_id = getPeerIdBySteam(key)
            transform_matrix, is_success = server.getPlayerPos(peer_id)
            x, y, z = matrix.position(transform_matrix)
            -- spawn point
            for enemy_team_id = 1, #SAVEDATA.TEAMS, 1 do
                if enemy_team_id ~= team_id then
                    ex, ez = SAVEDATA.TEAM_BASES[enemy_team_id][1], SAVEDATA.TEAM_BASES[enemy_team_id][3]
                    d = dist2(ex - x, ez - z)
                    if d < BASE_PROHIBIT_ZONE then
                        self_base = SAVEDATA.TEAM_BASES[team_id]
                        server.notify(peer_id, "WARNING",
                            string.format("You've been transport to your spawn point.", team_mp), 2)
                        server.setPlayerPos(peer_id, matrix.translation(self_base[1], self_base[2], self_base[3]))
                    elseif d < BASE_WARN_ZONE then
                        if value.STATUS == 1 then
                            server.notify(peer_id, "WARNING",
                                string.format("You are entering others spawn point.", team_mp), 2)
                            teamNotification(enemy_team_id, "SPAWN CAMPING",
                                string.format("%s is entering your spawn zone, position: (%.1f, %.1f)",
                                    server.getPlayerName(peer_id), x, z))
                        end
                        value.STATUS = 2
                    else
                        if value.STATUS==2 then
                            value.STATUS = 1
                        end
                    end
                end
            end
            -- map border
            isInMap = bPointInPolygen(SAVEDATA.BORDER, {x, z})
            if isInMap==false then
                self_base = SAVEDATA.TEAM_BASES[team_id]
                if value.STATUS~=3 then
                    server.notify(peer_id, "WARNING",
                            string.format("You're exiting game area.", team_mp), 2)
                    value.STATUS=3
                    value.RETURN_TIMER = 10
                elseif value.STATUS==3 then
                    -- server.announce("DEBUG", dump({value.RETURN_TIMER}))
                    if value.RETURN_TIMER<=0 then
                        server.setPlayerPos(peer_id, matrix.translation(self_base[1], self_base[2], self_base[3]))
                        value.STATUS=1
                    end
                    value.RETURN_TIMER = value.RETURN_TIMER - 1
                end
            elseif value.STATUS==3 then
                value.STATUS=1
            end
            -- cap point
            if GAME_START then
                for k, v in pairs(SAVEDATA.POINT_CAP) do
                    ex, ez = v[1], v[3]
                    d = dist2(ex - x, ez - z)
                    if d <= POINT_SIZE then
                        if team_id == 1 then cv = 2 else cv = -2 end
                        SAVEDATA.POINT_CAP[k][5] = v[5] + cv
                        if SAVEDATA.POINT_CAP[k][5] >= 100 then
                            SAVEDATA.POINT_CAP[k][4] = 1
                            SAVEDATA.POINT_CAP[k][5] = 100
                        elseif SAVEDATA.POINT_CAP[k][5] == 0 then
                            SAVEDATA.POINT_CAP[k][4] = 0
                        elseif SAVEDATA.POINT_CAP[k][5] <= -100 then
                            SAVEDATA.POINT_CAP[k][4] = 2
                            SAVEDATA.POINT_CAP[k][5] = -100
                        end
                        server.setPopupScreen(peer_id, value.UIBOT, "capture", true,
                            string.format("Capturing point: %d\n\n\n\n", SAVEDATA.POINT_CAP[k][5]), 0, -1)
                    end
                end
            end
        end
    end
end

function checkWin()
    if SAVEDATA.GAME_END == false then
        for key, value in pairs(SAVEDATA.TEAM_SCORES) do
            if value <= 0 then
                server.announce("[GAME]", string.format("Team %d lose!", key))
                SAVEDATA.GAME_END = true
                SAVEDATA.TEAM_SCORES[key] = 0
            end
        end
    end
end

function capEvent()
    if SAVEDATA.GAME_END == false then
        for key, value in pairs(SAVEDATA.POINT_CAP) do
            if value[4] ~= 0 then
                for tIndex, tValue in pairs(SAVEDATA.TEAM_SCORES) do
                    if tIndex ~= value[4] then
                        SAVEDATA.TEAM_SCORES[tIndex] = tValue - 1
                    end
                end
            end
        end
    end
end

function vehicleEvent()
    for team_id = 1, #SAVEDATA.TEAM_VEHICLES, 1 do
        team = SAVEDATA.TEAM_VEHICLES[team_id]
        for vehicle_id, vehicle_data in pairs(team) do
            VEHICLE_DATA, is_success = server.getVehicleData(vehicle_id)
            if VEHICLE_DATA == nil or type(VEHICLE_DATA) ~= "table" then
                team[vehicle_id]=nil
                return
            elseif VEHICLE_DATA.characters==nil or vehicle_data.HP==nil then
                -- server.announce("DEBUG", string.format("Vehicle %d deleted.", vehicle_id))
                -- server.announce("DEBUG", dump(VEHICLE_DATA))
                team[vehicle_id]=nil
                return
            end
            for char_id, tmp in pairs(VEHICLE_DATA.characters) do
                peer_id = getPeerIdByCharId(tmp)
                -- server.announce("DEBUG", dump({VEHICLE_DATA, peer_id, tmp}))
                player_info = getGamePlayer(peer_id)
                if player_info==0 then
                    return
                end
                server.setPopupScreen(peer_id, player_info.UITL, "vehicle", true,
                    string.format("HP: %.2f / %.2f\nCost: %.2f$", vehicle_data.HP, vehicle_data.MAXHP, vehicle_data.COST)
                    , 0.8, 0)
            end
        end
    end
end

function watchVehicleEvent()
    for vehicle_id, peer_id in pairs(SAVEDATA.WATCH_VEHICLES) do
        VEHICLE_DATA, is_success = server.getVehicleData(vehicle_id)
        if VEHICLE_DATA == nil or type(VEHICLE_DATA) ~= "table" or VEHICLE_DATA.transform==nil  then
            SAVEDATA.WATCH_VEHICLES[vehicle_id] = nil
            return
        end
        transform_matrix = VEHICLE_DATA.transform
        x, y, z=matrix.position(transform_matrix)
        if #VEHICLE_DATA.characters==0 then
            server.despawnVehicle(vehicle_id, true)
            SAVEDATA.WATCH_VEHICLES[vehicle_id] = nil
        elseif y<200 then
            VEHICLE_DATA.transform=matrix.translation(x, 400, z)
        end
    end
end


function flagEvent()
    for vehicle_id, spawn_pos in pairs(SAVEDATA.FLAGS) do
        VEHICLE_DATA, is_success = server.getVehicleData(vehicle_id)
        if VEHICLE_DATA == nil or type(VEHICLE_DATA) ~= "table" or VEHICLE_DATA.transform==nil  then
            SAVEDATA.FLAGS[vehicle_id] = nil
            return
        end
        transform_matrix = VEHICLE_DATA.transform
        x, y, z=matrix.position(transform_matrix)
        for team_id, team_base in pairs(SAVEDATA.TEAM_BASES) do
            ex, ez = team_base[1], team_base[3]
            d = dist2(ex - x, ez - z)
            if d < BASE_PROHIBIT_ZONE then
                self_base = SAVEDATA.TEAM_BASES[team_id]
                server.announce("GAME", string.format("Team %d captured a flag.", team_id))
                for tIndex, tValue in pairs(SAVEDATA.TEAM_SCORES) do
                    if tIndex ~= team_id then
                        SAVEDATA.TEAM_SCORES[tIndex] = tValue - 20
                    end
                end
                addDelay(1800, spawnFlag, {spawn_pos[1], spawn_pos[2], spawn_pos[3]})
                despawnFlag(vehicle_id)
                return
            end
        end
        addDelayMapObject(9, x, z, "Flag", 50, "Flag", 121, 91, 1, 255, 60)
    end
end

-- UI

function createUI(peer_id)
    ui_id = server.getMapID()
    for key, value in pairs(SAVEDATA.POINT_CAP) do
        server.addMapObject(peer_id, ui_id, 0, 9, value[1], value[3], 0, 0, 0, 0, string.format("Cap Point #%d", key),
            POINT_SIZE, "Capture this point to lower opponent's score.")
    end
    for key, value in pairs(SAVEDATA.TEAM_BASES) do
        server.addMapObject(peer_id, ui_id, 0, 11, value[1], value[3], 0, 0, 0, 0, string.format("Team #%d Base", key),
            BASE_WARN_ZONE, "Team base", 70, 70, 70, 255)
        server.addMapObject(peer_id, ui_id, 0, 11, value[1], value[3], 0, 0, 0, 0, string.format("Team #%d Base", key),
            BASE_PROHIBIT_ZONE, "Team base", 70, 70, 70, 255)
    end
    -- draw border
    for key, value in pairs(SAVEDATA.BORDER) do
        if key==1 then
            lastv = SAVEDATA.BORDER[#SAVEDATA.BORDER]
            lastm = matrix.translation(lastv[1],0,lastv[2])
        else
            lastm = tmpm
        end
        tmpm = matrix.translation(value[1],0,value[2])
        server.addMapLine(peer_id, ui_id, lastm, tmpm, 1, 70, 70, 70, 255)
    end
end

function refreshUI()
    for team_id = 1, #SAVEDATA.TEAMS, 1 do
        team = SAVEDATA.TEAMS[team_id]
        for key, value in pairs(team) do
            peer_id = getPeerIdBySteam(key)
            -- server.announce("DEBUG", dump({peer_id, ui_id}))
            server.removePopup(peer_id, value.UIBOT)
            server.removePopup(peer_id, value.UITOP)
            server.removePopup(peer_id, value.UIMID)
            server.removePopup(peer_id, value.UITL)
            tmp_str_map = { "Neutral", "Team 1", "Team 2"}  
            server.setPopupScreen(peer_id, value.UITOP, "Main board", true,
                string.format("\n\n\n\n\n[Team 1] %d\n[Team 2] %d\n[CP1] %s\n[CP2] %s", SAVEDATA.TEAM_SCORES[1], SAVEDATA.TEAM_SCORES[2], tmp_str_map[SAVEDATA.POINT_CAP[1][4] + 1], tmp_str_map[SAVEDATA.POINT_CAP[2][4] + 1]), 0
                , 1)
            if value.STATUS==3 then
                server.setPopupScreen(peer_id, value.UIMID, "Main board", true,
                    string.format("Return to area\nCount down\n%d sec", math.ceil(value.RETURN_TIMER)), 0
                    , 0)
            end
        end
    end
end

function addDelayMapObject(marker_type, x, z, label, radius, hover_label, r, g, b, a, time)
    ui_id = server.getMapID()
    for team_id = 1, #SAVEDATA.TEAMS, 1 do
        team = SAVEDATA.TEAMS[team_id]
        for key, value in pairs(team) do
            server.addMapObject(getPeerIdBySteam(key), ui_id, 0, marker_type, x, z, 0, 0, 0, 0, label, radius, hover_label, r, g, b, a)
            addDelay(time, server.removeMapObject, {getPeerIdBySteam(key), ui_id})
        end
    end
end

-- UTILS

function getPeerIdByCharId(charid)
    l = server.getPlayers()
    for i = 1, #l, 1 do
        if server.getPlayerCharacterID(l[i].id) == charid then
            return l[i].id
        end
    end
end

function getPeerIdBySteam(steamid)
    l = server.getPlayers()
    for i = 1, #l, 1 do
        if l[i].steam_id == steamid then
            return l[i].id
        end
    end
end

function getSteamByPeerId(peerid)
    l = server.getPlayers()
    for i = 1, #l, 1 do
        if l[i].id == peerid then
            return l[i].steam_id
        end
    end
end

function getPlayerTeam(peer_id)
    steamid = getSteamByPeerId(peer_id)
    for team_index = 1, #SAVEDATA.TEAMS, 1 do
        if SAVEDATA.TEAMS[team_index][steamid] ~= nil then
            return team_index
        end
    end
    return 0
end

function getGamePlayer(peer_id)
    steamid = getSteamByPeerId(peer_id)
    for team_index = 1, #SAVEDATA.TEAMS, 1 do
        if SAVEDATA.TEAMS[team_index][steamid] ~= nil then
            return SAVEDATA.TEAMS[team_index][steamid]
        end
    end
    return 0
end

function teamNotification(team_index, title, content, level)
    team = SAVEDATA.TEAMS[team_index]
    for key, value in pairs(team) do
        server.notify(getPeerIdBySteam(key), title, content, level)
    end
end

function artilleryStrike(x, y, z, radius, magnitude, next_count, time_to_next)
    randL, randA = math.random(radius), math.random(0,360)/math.pi-math.pi
    tx, tz=x+randL*math.cos(randA), z+randL*math.sin(randA)
    transform_matrix = matrix.translation(tx,y,tz)
    server.spawnExplosion(transform_matrix, magnitude)
    if next_count > 0 then
        addDelay(time_to_next, artilleryStrike, {x, y, z, radius, magnitude, next_count-1, time_to_next})
    end
end

function spawnFlag(x, y, z)
    vehicle_id = spawnAddonVehicle(2, x, y, z)
    SAVEDATA.FLAGS[vehicle_id] = {x, y, z}
end

function despawnFlag(vehicle_id)
    server.despawnVehicle(vehicle_id, true)
    SAVEDATA.FLAGS[vehicle_id] = nil
end

function spawnWatcherHeli(peer_id, x, y, z, delay)
    if x==nil or y==nil or z==nil then
        transform_matrix, is_success = server.getPlayerPos(peer_id)
        x, y, z = matrix.position(transform_matrix)
        y=200
    end
    if delay==nil then
        delay=60
    end
    spawnAddonVehicle(0, x, y, z)
    if is_success then
        addDelay(delay, getOnVehicle, {vehicle_id, peer_id, 1, true, 0})
    else
        server.notify(user_peer_id, "Failed", "Something wents wrong", 2)
    end
end

function spawnAddonVehicle(component_index, x, y, z)
    transform_matrix = matrix.translation(x, y, z)
    component_id = server.getLocationComponentData(ADDON_INDEX, LOCATION_INDEX, component_index).id
    vehicle_id, is_success = server.spawnAddonVehicle(transform_matrix, ADDON_INDEX, component_id)
    return vehicle_id
end

function addToWatchVehicles(vehicle_id, peer_id)
    SAVEDATA.WATCH_VEHICLES[vehicle_id] = peer_id
end

function getOnVehicle(vehicle_id, peer_id, seat_index, add_to_watch, retry_times)
    VEHICLE_DATA, is_success = server.getVehicleData(vehicle_id)
    if not is_success or VEHICLE_DATA==nil or VEHICLE_DATA.components==nil or VEHICLE_DATA.components.seats==nil or VEHICLE_DATA.components.seats[seat_index]==nil then
        if retry_times>=5 then
            server.notify(peer_id, "Failed", "Teleport range maybe too long", 2)
            if add_to_watch then
                addDelay(1800, addToWatchVehicles, {vehicle_id, peer_id})
            end
            return
        end
        addDelay(120, getOnVehicle, {vehicle_id, peer_id, 1, retry_times+1})
        return
    end
    seat_name = VEHICLE_DATA.components.seats[seat_index][1]
    -- server.setVehicleSeat(vehicle_id, seat_name, 0, 0, 0, 0, true, false, false, false, false, false)
    object_id, is_success = server.getPlayerCharacterID(peer_id)
    server.setCharacterSeated(object_id, vehicle_id, seat_name)
    if add_to_watch then
        addDelay(60, addToWatchVehicles, {vehicle_id, peer_id})
    end
end

function startsWith(str, start) return str:sub(1, #start) == start end

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

function dist2(x, y) return math.sqrt(x * x + y * y) end
function dist3(x, y, z) return math.pow(x * x + y * y + z * z, 1/3) end

function tablelength(T)
    local count = 0
    for _ in pairs(T) do count = count + 1 end
    return count
end

function bPointInPolygen(posList, posTarget)
    local polygenSides = #posList
    local flag = 0
    for i = 1, polygenSides do
        local Xa, Ya = posList[i][1], posList[i][2]
        local Xb, Yb = posList[i][1], posList[i][2]
        local Xp, Yp = posTarget[1], posTarget[2]
        if posList[i + 1] then
            Xb, Yb = posList[i + 1][1], posList[i + 1][2]
        else
            Xb, Yb = posList[1][1], posList[1][2]
        end
        if (Xp == Xa and Yp == Ya) or (Xp == Xb and Yp == Yb) then
            return true
        end
        if (Yp <= Yb and Yp >= Ya) or (Yp >= Yb and Yp <= Ya) then
            local tempX = Xa + (Yp - Ya) * (Xb - Xa) / (Yb - Ya)
            if Xp == tempX then
                return true
            end
            if Xp > tempX then
                flag = flag + 1
            end
        end
    end
    if flag % 2 == 0 then
        return false
    else
        return true
    end
end

function testFunc() server.announce("TEST", "beep!") end

-- Author: BKN
-- GitHub: https://github.com/BKN46
-- Workshop: https://steamcommunity.com/id/bkn46/myworkshopfiles/?appid=573090

--Code developed by BKN. Bilibili page: https://space.bilibili.com/522972--
--- Developed using LifeBoatAPI - Stormworks Lua plugin for VSCode - https://code.visualstudio.com/download (search "Stormworks Lua with LifeboatAPI" extension)
--- If you have any issues, please report them here: https://github.com/nameouschangey/STORMWORKS_VSCodeExtension/issues - by Nameous Changey

str = require("strings.lua")

function tablelength(T)
    local count = 0
    for _ in pairs(T) do count = count + 1 end
    return count
end

function getPlayerByCharId(charid)
    local l = server.getPlayers()
    for i = 1, #l, 1 do
        if server.getPlayerCharacterID(l[i].id) == charid then
            return l[i]
        end
    end
end

function getPlayerBySteamId(steamid)
    local l = server.getPlayers()
    for i = 1, #l, 1 do
        if l[i].steam_id == steamid then
            return l[i]
        end
    end
end

function getPlayerByPeerId(peerid)
    local l = server.getPlayers()
    for i = 1, #l, 1 do
        if l[i].id == peerid then
            return l[i]
        end
    end
end

function debugFunction(cmd, peer_id)
    local c = str.split(cmd, " ")
    local data = {
        players = PLAYERS,
        vehicles = VEHICLES,
        delay_events = DELAY_EVENTS,
        interval_events = INTERVAL_EVENTS,
    }
    local lastTmp, tmp, output, path = nil, data, "", ""
    for i, v in ipairs(c) do
        if not tmp then server.notify(peer_id, "[Debug]", "Data not found in: " .. str.dumpBase(tmp), 2) end
        tmp = data[v]
        path = path .. "." .. v
        if type(tmp) == "function" then
            output = tmp(lastTmp, table.unpack(c, i + 1))
            server.notify(peer_id, "[Debug]", "Function exec return: " .. str.dump(output), 4)
            return
        elseif i == #c then
            server.notify(peer_id, "[Debug]", "Data " .. path .. " :" .. str.dump(tmp), 4)
            return
        end
        lastTmp = tmp
    end
end

function sendToServer(path, data)
    if data==nil then
        server.httpGet(5588, string.format("/%s", path))
    else
        server.httpGet(5588, string.format("/%s?data=%s", path, str.dump(data)))
    end
end

-- Author: BKN
-- GitHub: https://github.com/BKN46
-- Workshop: https://steamcommunity.com/id/bkn46/myworkshopfiles/?appid=573090

--Code developed by BKN. Bilibili page: https://space.bilibili.com/522972--
--- Developed using LifeBoatAPI - Stormworks Lua plugin for VSCode - https://code.visualstudio.com/download (search "Stormworks Lua with LifeboatAPI" extension)
--- If you have any issues, please report them here: https://github.com/nameouschangey/STORMWORKS_VSCodeExtension/issues - by Nameous Changey

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

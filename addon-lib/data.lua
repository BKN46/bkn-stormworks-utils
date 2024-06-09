-- Author: BKN
-- GitHub: https://github.com/BKN46
-- Workshop: https://steamcommunity.com/id/bkn46/myworkshopfiles/?appid=573090

--Code developed by BKN. Bilibili page: https://space.bilibili.com/522972--
--- Developed using LifeBoatAPI - Stormworks Lua plugin for VSCode - https://code.visualstudio.com/download (search "Stormworks Lua with LifeboatAPI" extension)
--- If you have any issues, please report them here: https://github.com/nameouschangey/STORMWORKS_VSCodeExtension/issues - by Nameous Changey

utils = require("utils.lua")

function registPlayer(peer_id)
    if not PLAYERS then
        ---@type table<number, Player>
        PLAYERS = {}
    end

    local playerInfo = utils.getPlayerByPeerId(peer_id)

    ---@class Player
    local player = {
        steamId = playerInfo.steam_id,
        peerId = playerInfo.id,
        name = playerInfo.name,
        team = nil,
        role = nil,
        money = 0,
        ---@type table<string, UI>
        UIs = {},
        addUI = function(self, name, ui)
            self.UIs[name] = ui
        end,
        getObjectId = function(self)
            local object_id, _ = server.getPlayerCharacterID(self.peerId)
            return object_id
        end,
        getObjectData = function (self)
            return server.getObjectData(self:getObjectId())
        end,
        kill = function (self)
            server.killCharacter(self:getObjectId())
        end,
        revive = function (self)
            server.reviveCharacter(self:getObjectId())
        end,
        getVehicles = function (self)
            local vehicles = {}
            for k, v in pairs(VEHICLES) do
                if v.owner == self.peerId then
                    table.insert(vehicles, v)
                end
            end
            return vehicles
        end,
    }

    PLAYERS[peer_id] = player
    return player
end

function updatePlayers()
    if not PLAYERS then
        return
    end
    for k, v in pairs(PLAYERS) do
        
    end
end

function saveGame()
    local data = {
        players = PLAYERS,
        vehicles = VEHICLES,
        delay_events = DELAY_EVENTS,
        interval_events = INTERVAL_EVENTS,
    }
    g_savedata = data
    return data
end

function loadGame()
    if g_savedata then
        PLAYERS = g_savedata.players
        VEHICLES = g_savedata.vehicles
        DELAY_EVENTS = g_savedata.delay_events
        INTERVAL_EVENTS = g_savedata.interval_events
    end
end

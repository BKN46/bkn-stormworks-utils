-- Author: BKN
-- GitHub: https://github.com/BKN46
-- Workshop: https://steamcommunity.com/id/bkn46/myworkshopfiles/?appid=573090

--Code developed by BKN. Bilibili page: https://space.bilibili.com/522972--
--- Developed using LifeBoatAPI - Stormworks Lua plugin for VSCode - https://code.visualstudio.com/download (search "Stormworks Lua with LifeboatAPI" extension)
--- If you have any issues, please report them here: https://github.com/nameouschangey/STORMWORKS_VSCodeExtension/issues - by Nameous Changey

timers = require("timers.lua")

function createUI(peerId, title, x, y)
    if not ALL_UI then
        ---@type table<number, UI>
        ALL_UI = {}
    end
    local id = server.getMapID()
    ---@class UI
    local ui = {
        id = id,
        title = title,
        x = x,
        y = y,
        refresh = function(self, text)
            server.setPopupScreen(
                peerId,
                self.id,
                self.title,
                true,
                text,
                self.x,
                self.y
            )
        end,
        remove = function(self)
            server.removePopup(peerId, self.id)
        end,
    }
    table.insert(ALL_UI, ui)
    return ui
end

function addDelayMapObject(peerId, marker_type, x, z, label, radius, hover_label, r, g, b, a, time)
    local ui_id = server.getMapID()
    server.addMapObject(peerId, ui_id, 0, marker_type, x, z, 0, 0, 0, 0, label, radius, hover_label, r, g, b, a)
    timers.addDelay(time, server.removeMapObject, {peerId, ui_id})
end

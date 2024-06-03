-- Author: BKN
-- GitHub: https://github.com/BKN46
-- Workshop: https://steamcommunity.com/id/bkn46/myworkshopfiles/?appid=573090

--Code developed by BKN. Bilibili page: https://space.bilibili.com/522972--
--- Developed using LifeBoatAPI - Stormworks Lua plugin for VSCode - https://code.visualstudio.com/download (search "Stormworks Lua with LifeboatAPI" extension)
--- If you have any issues, please report them here: https://github.com/nameouschangey/STORMWORKS_VSCodeExtension/issues - by Nameous Changey

function doDelay(time, do_func, param)
    if not DELAY_EVENTS then
        DELAY_EVENTS = {}
    end
    table.insert(DELAY_EVENTS, {
        TIME = time,
        DO = do_func,
        PARAM = param,
    })
end

function updateDelay()
    if not DELAY_EVENTS then
        return
    end
    for k, v in pairs(DELAY_EVENTS) do
        if v.TIME > 0 then
            DELAY_EVENTS[k].TIME = DELAY_EVENTS[k].TIME - 1
        else
            v.DO(table.unpack(v.PARAM))
            DELAY_EVENTS[k] = nil
        end
    end
end

function addInterval(time, do_func, param)
    if not INTERVAL_EVENTS then
        INTERVAL_EVENTS = {}
    end
    table.insert(INTERVAL_EVENTS, {
        TIME = time,
        DO = do_func,
        PARAM = param,
    })
end

function updateInterval()
    if not INTERVAL_EVENTS then
        return
    end
    for k, v in pairs(INTERVAL_EVENTS) do
        if v.TIME > 0 then
            INTERVAL_EVENTS[k].TIME = INTERVAL_EVENTS[k].TIME - 1
        else
            v.DO(table.unpack(v.PARAM))
            INTERVAL_EVENTS[k].TIME = v.TIME
        end
    end
end

function deleteInterval(do_func)
    if not INTERVAL_EVENTS then
        return
    end
    for k, v in pairs(INTERVAL_EVENTS) do
        if v.DO == do_func then
            INTERVAL_EVENTS[k] = nil
        end
    end
end

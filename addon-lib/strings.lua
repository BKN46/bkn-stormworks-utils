-- Author: BKN
-- GitHub: https://github.com/BKN46
-- Workshop: https://steamcommunity.com/id/bkn46/myworkshopfiles/?appid=573090

--Code developed by BKN. Bilibili page: https://space.bilibili.com/522972--
--- Developed using LifeBoatAPI - Stormworks Lua plugin for VSCode - https://code.visualstudio.com/download (search "Stormworks Lua with LifeboatAPI" extension)
--- If you have any issues, please report them here: https://github.com/nameouschangey/STORMWORKS_VSCodeExtension/issues - by Nameous Changey

function dump(b) 
    if type(b) == 'table' then local d = '{ '
        for e, f in pairs(b) do if type(e) ~= 'number' then e = '"' ..
                    e .. '"'
            end
            d = d .. '' .. e .. ': ' .. dump(f) .. ', '
        end
        return d .. '} '
    elseif type(b) == 'function' then return '<function>'
    else return tostring(b) end
end

function split(a, b) if b == nil then b = "%s" end
    local c = {}
    for d in string.gmatch(a, "([^" .. b .. "]+)") do table.insert(c, d) end
    return c
end

function startsWith(str, start) return str:sub(1, #start) == start end

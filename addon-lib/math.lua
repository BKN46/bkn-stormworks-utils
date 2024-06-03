-- Author: BKN
-- GitHub: https://github.com/BKN46
-- Workshop: https://steamcommunity.com/id/bkn46/myworkshopfiles/?appid=573090

--Code developed by BKN. Bilibili page: https://space.bilibili.com/522972--
--- Developed using LifeBoatAPI - Stormworks Lua plugin for VSCode - https://code.visualstudio.com/download (search "Stormworks Lua with LifeboatAPI" extension)
--- If you have any issues, please report them here: https://github.com/nameouschangey/STORMWORKS_VSCodeExtension/issues - by Nameous Changey

function dist2(x, y) return math.sqrt(x * x + y * y) end

function dist3(x, y, z) return (x * x + y * y + z * z) ^ (1/2) end

function pointInPolygen(posList, posTarget)
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

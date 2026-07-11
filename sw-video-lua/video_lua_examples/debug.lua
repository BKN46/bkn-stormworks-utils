ready = false
last_frame = -1
pixels = nil

function onTick()
    if not ready then
        local ok, err = video.init(32, 32, "gray")
        ready = ok
        if not ok then
            output.setNumber(1, -1)
            return
        end
    end

    if not video.isConnected() then
        output.setNumber(1, 0)
        return
    end

    if not video.isReady() then
        output.setNumber(1, -3)
        return
    end

    local frame_id = video.getInfo()
    if frame_id == last_frame then
        return
    end
    last_frame = frame_id

    pixels, err = video.getGray()
    -- pixels, err = video.getRGB()
    if not pixels then
        output.setNumber(1, -2)
        return
    end

    output.setNumber(2, frame_id)
end

function onDraw()
    height=screen.getHeight()
    if pixels == nil then return end
    for y = 1, #pixels do
        for x = 1, #pixels[y] do
            local gray = pixels[y][x][3]
            screen.setColor(gray,gray,gray)
            -- local rgb = pixels[y][x][3]
            -- screen.setColor(rgb[1],rgb[2],rgb[3])
            screen.drawRectF(x, height-y+1, 1, 1)
        end
    end
	screen.setColor(0,255,0)
	screen.drawTextBox(2,2,60,60,string.format('h: %d\nw: %d\n%s',#pixels,#pixels[1],video.isReady()))
end

function dump(b)if type(b)=='table'then local d='{ 'for e,f in pairs(b)do if type(e)~='number'then e='"'..e..'"'end;d=d..'['..e..'] = '..dump(f)..',\n'end;return d..'} 'else return tostring(b)end end

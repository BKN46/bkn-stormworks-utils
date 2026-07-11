capture_ready = false
initialized = false
last_frame_id = -1
frame_pixels = nil

function onTick()
    if video == nil then
        output.setNumber(1, -10)
        return
    end

    if not initialized then
        local ok, err = video.init(64, 64, "rgb")
        initialized = ok
        if not ok then
            output.setNumber(1, -11)
            return
        end
    end

    if not video.isConnected() then
        output.setNumber(1, 1)
        return
    end

    if not video.isReady() then
        output.setNumber(1, 2)
        return
    end

    local frame_id, width, height, mode = video.getInfo()
    local bytes, err = video.getPackedRGB()
    if not bytes then
        output.setNumber(1, -12)
        return
    end

    frame_pixels = bytes
    last_frame_id = frame_id or last_frame_id
    capture_ready = true

    output.setNumber(1, 3)
    output.setNumber(2, last_frame_id)
    output.setNumber(3, width or -1)
    output.setNumber(4, height or -1)
end

function onDraw()
    local sw = screen.getWidth()
    local sh = screen.getHeight()

    screen.setColor(0, 0, 0)
    screen.drawClear()

    if not capture_ready or frame_pixels == nil then
        return
    end

    for y = 1, 64 do
        local y0 = math.floor((y - 1) * sh / 64)
        local y1 = math.floor(y * sh / 64)
        local rh = math.max(1, y1 - y0)

        for x = 1, 64 do
            local offset = ((y - 1) * 64 + (x - 1)) * 3
            local r = frame_pixels[offset + 1]
            local g = frame_pixels[offset + 2]
            local b = frame_pixels[offset + 3]
            if r ~= nil and g ~= nil and b ~= nil then
                local x0 = math.floor((x - 1) * sw / 64)
                local x1 = math.floor(x * sw / 64)
                local rw = math.max(1, x1 - x0)
                screen.setColor(r, g, b)
                screen.drawRectF(x0, y0, rw, rh)
            end
        end
    end
end

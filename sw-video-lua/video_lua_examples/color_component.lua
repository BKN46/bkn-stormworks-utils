ready = false
last_frame = -1

function onTick()
    if not ready then
        local ok, err = video.init(64, 36, "rgb")
        ready = ok
        if not ok then
            output.setNumber(1, -1)
            return
        end
    end

    if not video.isConnected() or not video.isReady() then
        output.setNumber(1, 0)
        return
    end

    local frame_id = video.getInfo()
    if frame_id == last_frame then
        return
    end
    last_frame = frame_id

    local pixels, err = video.getRGB()
    if not pixels then
        output.setNumber(1, -2)
        return
    end

    local red_pixels = 0
    for y = 1, #pixels do
        for x = 1, #pixels[y] do
            local rgb = pixels[y][x][3]
            local r = rgb[1]
            local g = rgb[2]
            local b = rgb[3]

            if r > 180 and g < 80 and b < 80 then
                red_pixels = red_pixels + 1
            end
        end
    end

    output.setNumber(1, red_pixels)
end

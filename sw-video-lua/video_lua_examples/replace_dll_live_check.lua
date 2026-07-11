initialized = false
last_frame = -1
frame_changes = 0

function onTick()
    if video == nil then
        output.setNumber(1, -10)
        return
    end

    if not initialized then
        local ok, err = video.init(64, 36, "rgb")
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
    if frame_id ~= nil and frame_id ~= last_frame then
        frame_changes = frame_changes + 1
    end
    output.setNumber(2, frame_id or -1)
    output.setNumber(5, width or -1)
    output.setNumber(6, height or -1)
    output.setNumber(7, frame_changes)

    local bytes, err = video.getPackedRGB()
    if not bytes then
        output.setNumber(1, -12)
        return
    end

    local sample = 0
    local count = 0
    for i = 1, #bytes, 18 do
        sample = sample + bytes[i] + (bytes[i + 1] or 0) + (bytes[i + 2] or 0)
        count = count + 1
    end

    output.setNumber(1, 3)
    output.setNumber(3, sample)
    output.setNumber(4, count)
    last_frame = frame_id
end

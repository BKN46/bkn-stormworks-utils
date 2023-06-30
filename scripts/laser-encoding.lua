--[[
	Receiver state machine:
		1. wait for sync
		2. data transmission
	Data bit design (9hz):
		sync: 1 1 1 1 1 1
		data: 0 x x x x 0
	data must flip and write
]]

-- sender
TIMER,SYNC_TIMER,WIN_LEN=1,1,6
SYNC_PACK={1,1,1,1,1,1}
SEND_PACK={0,1,1,1,1,0}

function onTick()
	SB=output.setBool
	if SYNC_TIMER<=WIN_LEN then
		SB(1,SYNC_PACK[TIMER])
	else
		SB(1,SEND_PACK[TIMER])
	end
	TIMER=(TIMER%WIN_LEN)+1
	SYNC_TIMER=(SYNC_TIMER%60)+1
end

-- receiver
TIMER,WIN_LEN=1,6
STATE=1
CMD_CACHE,LAST_CMD={},{}

function onTick()
	GB=input.getBool
	if STATE==1 then
		table.insert(CMD_CACHE, GB(1))
		if #CMD_CACHE==WIN_LEN then
			if table.concat(CMD_CACHE)==string.rep("1",WIN_LEN) then
				STATE=2
				CMD_CACHE={}
			else
				table.remove(CMD_CACHE, 1)
			end
		end
	elseif STATE==2 then
		table.insert(CMD_CACHE, GB(1))
		if #CMD_CACHE==WIN_LEN then
			if table.concat(CMD_CACHE)==string.rep("1",WIN_LEN) then
				CMD_CACHE={}
			elseif CMD_CACHE[1]==1 or CMD_CACHE[WIN_LEN]==1 then
				STATE=1
				LAST_CMD={}
			else
				LAST_CMD=CMD_CACHE
				CMD_CACHE={}
			end
		end
	end
end

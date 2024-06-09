PREDICT_OFFSET = 0
PPL,PSL,PAL = 8, 5, 5
PPLm,PSLm,PALm = 7, 4, 3
PPLM,PSLM,PALM = 30, 10, 10
p={0,0,0}

function onTick()
	isTarget=input.getBool(1)
	output.setBool(1,isTarget)
	if not isTarget then return end
	dist, azim, elev = input.getNumber(1), input.getNumber(2), input.getNumber(3)
	x = dist * math.cos(elev * math.pi * 2) * math.cos(azim * math.pi * 2)
	y = dist * math.cos(elev * math.pi * 2) * math.sin(azim * math.pi * 2)
	z = dist * math.sin(elev * math.pi * 2)

	p = {x, y, z}
	p = kalman(1, p, 0.05)
	P = pushP(P, p, PPL)

	P_m = pushP(P_m, avgList(P), PSL)
	p = avgList(P)

	if #P_m >= PSL then
		spd = avgList(diffList(P_m))
	else
		spd = {0, 0, 0}
	end
	spd = kalman(2, spd, 0.1)

	S_m = pushP(S_m, spd, PAL)
	if #S_m >= PAL then
		acc = avgList(diffList(S_m))
	else
		acc = {0, 0, 0}
	end
	acc = kalman(3, acc, 0.1)


	p = addP(p, mulP(spd, -PPL/2))
	if PREDICT_OFFSET > 0 then
		for i = 1, PREDICT_OFFSET do
			spd = addP(spd, acc)
			p = addP(p, mulP(spd, -1))
		end
	end

	output.setNumber(1, p[1])
	output.setNumber(2, p[2])
	output.setNumber(3, p[3])

	if #P < PPL then
	elseif absP(acc) > 0.005 then
		PPL = math.max(PPLm, PPL - 1)
		PSL = math.max(PSLm, PSL - 1)
		PAL = math.max(PALm, PAL - 1)
	else
		PPL = math.min(PPLM, PPL + 1)
		PSL = math.min(PSLM, PSL + 1)
		PAL = math.min(PALM, PAL + 1)
	end
end

function minMaxL(l)
	local min, max = l[1], l[1]
	for i = 2, #l do
		min = math.min(min, l[i])
		max = math.max(max, l[i])
	end
	return min, max
end

function avgP(p1, p2)
	return {(p2[1] + p1[1]) / 2, (p2[2] + p1[2]) / 2, (p2[3] + p1[3]) / 2}
end

function diffP(p1, p2)
	return {p2[1] - p1[1], p2[2] - p1[2], p2[3] - p1[3]}
end

function diffList(l)
	local r = {}
	for i = 2, #l do
		table.insert(r, diffP(l[i], l[i-1]))
	end
	return r
end

function mulP(p, n)
	return {p[1] * n, p[2] * n, p[3] * n}
end

function addP(p1, p2)
	return {p1[1] + p2[1], p1[2] + p2[2], p1[3] + p2[3]}
end

function divideP(p, n)
	return {p[1] / n, p[2] / n, p[3] / n}
end

function absP(p)
	return math.sqrt(p[1] * p[1] + p[2] * p[2] + p[3] * p[3])
end

function pushP(l, p, max)
	if not l then l = {} end
	if #l >= max then
		table.remove(l, 1)
	end
	table.insert(l, p)
	return l
end

function angleP(p1, p2)
	return {
		math.acos(p1[1] * p2[1] + p1[2] * p2[2] + p1[3] * p2[3]),
		math.acos(p1[2] * p2[3] - p1[3] * p2[2]),
		math.acos(p1[3] * p2[1] - p1[1] * p2[3]),
	}
end

function lowPassP(prevP, newP, ratio)
	if not prevP then
		return newP
	end
	return {
		newP[1] * ratio + prevP[1] * (1-ratio),
		newP[2] * ratio + prevP[2] * (1-ratio),
		newP[3] * ratio + prevP[3] * (1-ratio)
	}
end

function avgList(list)
	local sum = {0, 0, 0}
	for i = 1, #list do
		sum = addP(sum, list[i])
	end
	return divideP(sum, #list)
end

function kalman(i, p, q)
	if not kMtx then kMtx = {} end
	if not kMtx[i] then
		kMtx[i] = {{}, {}, {}}
		for j = 1, 3, 1 do
			kMtx[i][j][1] = p[j]
			kMtx[i][j][2] = q
			kMtx[i][j][3] = 1
			kMtx[i][j][4] = 1/(1+q)
		end
	else
		for j = 1, 3, 1 do
			kMtx[i][j][4] = kMtx[i][j][3]/(kMtx[i][j][3]+kMtx[i][j][2])
			kMtx[i][j][1] = kMtx[i][j][1]+kMtx[i][j][4]*(p[j]-kMtx[i][j][1])
			kMtx[i][j][3] = (1-kMtx[i][j][4])
		end
	end
	return {
		kMtx[i][1][1],
		kMtx[i][2][1],
		kMtx[i][3][1],
	}
end
	
function onDraw()
	screen.setColor(0,255,0)
	screen.drawText(2,2,string.format("%.2f",p[1]))
	screen.drawText(2,12,string.format("%.2f",p[2]))
	screen.drawText(2,22,string.format("%.2f",p[3]))
	if isTarget then screen.drawText(2,32,"Locked") end
end
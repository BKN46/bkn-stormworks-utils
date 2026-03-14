STATE=0
N_FIRE=1
IB={false,false,false,false,false}
IN={0,0,0,0,0,0,0,}
OB={false,false,false,false,false,false,false,false}
ON={0,0,0,0,0,0,0,}
fL=false

DELAY_EVENTS = {}

TIMER,TIMER_HOLDER=0,false

function onTick()
	I,O=input,output
	GN,GB=I.getNumber,I.getBool
	SN,SB=O.setNumber,O.setBool

	for i=1,32,1 do IB[i]=GB(i) end
	for i=1,32,1 do IN[i]=GN(i) end

	STATE_MACHINE()

	for i,v in ipairs(OB) do SB(i,v) end
	for i,v in ipairs(ON) do SN(i,v) end

	doDelay()
end

function STATE_MACHINE()
	-- IN: turn, tilt, turretTurn, loaderTurn, ammo, CannonPosition
	-- IB: fire, debug, r1, r2, r3
	-- ON: turretTurn, slider, loaderTurn, turretTilt, loaderTilt, loaderLower
	-- OB: f1, b1, f2, b2, f3, b3, cl, cr

	if STATE==0 then
		OB[N_FIRE*2-1]=IB[1]
		if IB[1] and not fL then
			addDelay(1, sState, {1})
			addDelay(1, breach, {N_FIRE, true})
			addDelay(1, nextRound, {})
			fL=true
		elseif not IB[1] then
			fL=false
		end
	else
		OB[1],OB[3],OB[5],fL=false,false,false,false
	end

	RAIL_TO(N_FIRE)
	ON[4]=IN[2]

	if STATE==0 then
		ON[1]=turT(IN[3], IN[1])
		ON[2]=-1
		ON[3]=turT(IN[4], IN[1])
		ON[5]=-IN[2]
		ON[6]=0
	elseif STATE==1 then
		ON[1]=turT(IN[3], IN[1])
		ON[2]=1
		loaderTurn()
		ON[5]=0
		ON[6]=0
		NEXT_STATE(15,2)
	elseif STATE==2 then
		ON[1]=turT(IN[3], IN[1])
		ON[2]=1
		loaderTurn()
		ON[5]=0
		ON[6]=-1
		NEXT_STATE(60,3)
	elseif STATE==3 then
		ON[1]=turT(IN[3], IN[1])
		ON[2]=1
		loaderTurn()
		ON[5]=-IN[2]
		ON[6]=0
		NEXT_STATE(15,4)
	elseif STATE==4 then
		ON[1]=turT(IN[3], IN[1])
		ON[2]=-1
		ON[3]=turT(IN[4], IN[1])
		ON[5]=-IN[2]
		ON[6]=0
		NEXT_STATE(15,0)
	end
end

function loaderTurn()
	--O_NUM[3]=turretTurn(I_NUM[4], 0.25*(I_NUM[5]-1))
	local t = N_FIRE * 0.25 + -0.5
	ON[3]=turT(IN[4], t)
end

function nextRound() N_FIRE=(N_FIRE%3)+1 end
function sState(n) STATE=n end

function breach(n,o)
	OB[n*2]=o
	if o then addDelay(250, breach, {n, false}) end
end

function NEXT_STATE(time, state)
	if TIMER==0 and not TIMER_HOLDER then
		TIMER,TIMER_HOLDER=time,true
	elseif TIMER==0 then
		STATE,TIMER_HOLDER=state,false
	else
		TIMER=TIMER-1
	end
end

function RAIL_TO(t)
	if t==1 then b=0 elseif t==2 then b=0.75 elseif t==3 then b=-0.75 end
	OB[7],OB[8]=IN[6]-b>0.05,IN[6]-b<-0.05
end
	
function onDraw()
	S=screen
	Text,TextBox,Color,Line,RectF,Rect,Circle,CircleF,Triangle,TriangleF=S.drawText,S.drawTextBox,S.setColor,S.drawLine,S.drawRectF,S.drawRect,S.drawCircle,S.drawCircleF,S.drawTriangle,S.drawTriangleF
	w,h=S.getWidth(),S.getHeight()
	if STATE==0 then Color(0,255,0) else Color(0,0,0) end
	RectF(0,0,w,h)
	Color(255,255,255)
	Text(2,2,string.format("R %d",N_FIRE))
	Text(2,8,string.format("S %d",STATE))
	-- Text(2,14,string.format("R %d",IN[3]))
end

function turT(x,y) return -((x-y)-sgn(x-y)*math.floor(math.abs(x-y)+0.5))*5 end
function sgn(x) if x>=0 then return 1 else return -1 end end

function doDelay()
	if not DELAY_EVENTS then DELAY_EVENTS = {} end
	for k, v in pairs(DELAY_EVENTS) do
		if v.TIME > 0 then
			DELAY_EVENTS[k].TIME = DELAY_EVENTS[k].TIME - 1
		else
			v.DO(table.unpack(v.PARAM))
			DELAY_EVENTS[k] = nil
		end
	end
end
function addDelay(time, do_func, param)
	if not DELAY_EVENTS then DELAY_EVENTS = {} end
	table.insert(DELAY_EVENTS, {
		["TIME"] = time,
		["DO"] = do_func,
		["PARAM"] = param,
	})
end
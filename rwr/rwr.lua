data,target,turn={},0,-1
detectThreshold=property.getNumber("detectThreshold")
tmpList,targetList={},{}

function onTick()
	I,O=input,output
	GN,GB=I.getNumber,I.getBool
	SN,SB=O.setNumber,O.setBool
	P=property
	PN,PB,PT=P.getNumber,P.getBool,P.getText

	nextTurn=GN(1)%1<turn
	rwr,turn,compass=GB(1),GN(1)%1,GN(2)
	if nextTurn then
		detected,tmpList=0,{}
		for i,v in pairs(data) do
			if not v[2] and detected==0 then
				tmpList[#tmpList+1]={v[1]+compass}
				detected=detected+1
			elseif not v[2] and detected>0 then
				table.insert(tmpList[#tmpList],v[1]+compass)
				detected=detected+1
			elseif v[2] and detected>0 then
				detected=0
			end
		end
		if detected==#data then tmpList={}
		elseif detected>0 then
			for i,v in pairs(tmpList[1]) do
				table.insert(tmpList[#tmpList],v)
			end
			table.remove(tmpList,1)
		end
		targetList={}
		for i,v in pairs(tmpList) do
			if #v>=detectThreshold then
				local res=reduce(v,add,0)/#v-compass
				table.insert(targetList,res)
			end
		end
		data={}
	else
		table.insert(data,{turn+P.getNumber("shieldOffset"),rwr})
	end
	SN(1,1)
end

function onDraw()
	S=screen
	Text,TextBox,Color,Line,RectF,Rect,Circle,CircleF,Triangle,TriangleF=S.drawText,S.drawTextBox,S.setColor,S.drawLine,S.drawRectF,S.drawRect,S.drawCircle,S.drawCircleF,S.drawTriangle,S.drawTriangleF
	w,h=S.getWidth(),S.getHeight()

	Color(255,255,255)
	-- TextBox(0,0,w,h,dump(targetList))
	-- Text(2,2,turn)
	-- Text(2,10,target)
	-- Text(2,18,#data)

	local r=20
	Circle(w/2,h/2+r,r)
	for i,v in pairs(data) do
		if v[2] then Color(255,0,0)else Color(0,255,0) end
		RectF(i+1,26,1,5)
		Line(w/2,h/2+r,w/2-r*math.cos(v[1]*math.pi*2),h/2+r-r*math.sin(v[1]*math.pi*2))
	end
	Color(255,255,255)
	for i,v in pairs(targetList) do
        Text(2,2+i*7,string.format("radar-%d: %.6f",i,v))
        Line(w/2,h/2+r,w/2-r*math.cos(v*math.pi*2),h/2+r-r*math.sin(v*math.pi*2))
    end
end

function dump(b)if type(b)=='table'then local d='{ 'for e,f in pairs(b)do if type(e)~='number'then e='"'..e..'"'end;d=d..'['..e..'] = '..dump(f)..',\n'end;return d..'} 'else return tostring(b)end end
function reduce(b,c,d) local e=d;for f,g in ipairs(b)do if 1==f and not d then e=g else e=c(e,g)end end;return e end
function add(a,b) return a+b end
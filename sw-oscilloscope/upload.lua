INTV=30
UPLOAD_TABLE={}

function onTick()
	I,O=input,output
	GN,GB=I.getNumber,I.getBool
	P=property
	PN,PB,PT=P.getNumber,P.getBool,P.getText
	num_table=split(PT("Which number ouput to monitor(split by ,)"),",")
	bool_table=split(PT("Which bool ouput to monitor(split by ,)"),",")
	value=""
	for i=1,32,1 do
		if num_table[i]~=nil then
			if i==1 then
				value=string.format("%.6f", GN(i))
			else
				value=value..string.format(",%.6f", GN(i))
			end
		end
	end
	for i=1,32,1 do
		if bool_table[i]~=nil then
			if GB(i) then
				tmp=1
			else
				tmp=0
			end
			value=value..string.format(",%d", tmp)
		end
	end
	table.insert(UPLOAD_TABLE, value)

	if INTV==0 then
		upload_value=table.concat(UPLOAD_TABLE, "|||")
		async.httpGet(5588, string.format("/send?value=%s", upload_value))
		UPLOAD_TABLE={}
		INTV=30
	else
		INTV=INTV-1
	end
end

function onDraw()
	S=screen
	Text,TextBox,Color,Line,RectF,Rect,Circle,CircleF,Triangle,TriangleF=S.drawText,S.drawTextBox,S.setColor,S.drawLine,S.drawRectF,S.drawRect,S.drawCircle,S.drawCircleF,S.drawTriangle,S.drawTriangleF
	w,h=S.getWidth(),S.getHeight()
	TextBox(0,0,w,s,dump(num_table))
end

function httpReply(port, request_body, response_body) end

function split(a,b)if b==nil then b="%s"end;local c={}for d in string.gmatch(a,"([^"..b.."]+)")do table.insert(c,d)end;return c end

function dump(b)if type(b)=='table'then local d='{ 'for e,f in pairs(b)do if type(e)~='number'then e='"'..e..'"'end;d=d..'['..e..'] = '..dump(f)..',\n'end;return d..'} 'else return tostring(b)end end

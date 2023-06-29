INTV=30
UPLOAD_TABLE={}

function onTick()
	I,O=input,output
	GN,GB=I.getNumber,I.getBool
	value=""
	for i=1,32,1 do
		if i==1 then
			value=string.format("%.6f", GN(i))
		else
			value=value..string.format(",%.6f", GN(i))
		end
	end
	for i=1,32,1 do
		if GB(i) then
			tmp=1
		else
			tmp=0
		end
		value=value..string.format(",%d", tmp)
	end
	table.insert(UPLOAD_TABLE, value)

	if INTV==0 then
		upload_value=table.concat(UPLOAD_TABLE, "\n")
		async.httpGet(5588, string.format("/send?value=%s", upload_value))
		UPLOAD_TABLE={}
		INTV=30
	else
		INTV=INTV-1
	end
end

function httpReply(port, request_body, response_body) end

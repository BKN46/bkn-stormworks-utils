iB=input.getBool
iN=input.getNumber
oN=output.setNumber
oB=output.setBool
m=math
pi=m.pi
pi2=2*pi
s=m.sin
c=m.cos
as=m.asin
at=m.atan

function matrixEularRotation(E) qx,qy,qz=E[1],E[2],E[3] return {{c(qy)*c(qz),c(qx)*c(qy)*s(qz)+s(qx)*s(qy),s(qx)*c(qy)*s(qz)-c(qx)*s(qy)},{-s(qz),c(qx)*c(qz),s(qx)*c(qz)},{s(qy)*c(qz),c(qx)*s(qy)*s(qz)-s(qx)*c(qy),s(qx)*s(qy)*s(qz)+c(qx)*c(qy)}} end

function matrixTranspose(M)
	N={{},{},{}}
	for i=1,3 do
		for j=1,3 do
			N[i][j]=M[j][i]
		end
	end
	return N
end
function matrixMultVector(M,v)
	u={}
	for i=1,3 do
		_=0
		for j=1,3 do
			_=_+M[j][i]*v[j]
		end
		u[i]=_
	end
	return u
end
function matrixInnerProduction(u,v)
	_=0
	for i=1,3 do
		_=_+u[i]*v[i]
	end
	return _
end


function onTick()
	PO={iN(1),iN(3),iN(2)}
	Eu={iN(4),iN(6),iN(5)}
	PT={iN(15),iN(16),iN(17)}

	B=matrixEularRotation(Eu)
	-- b=matrixTranspose(B)

	PN={}
    for i=1,3 do
        PN[i]=PT[i]-PO[i]
    end
    PN=matrixMultVector(B,PN)

	targetPos={PN[1],PN[2],PN[3]}
	oN(1,PN[1])
	oN(2,PN[2])
	oN(3,PN[3])
end

function onDraw()
	screen.setColor(0,255,0)
	screen.drawText(2,2,string.format("%.2f",PN[1]))
	screen.drawText(2,12,string.format("%.2f",PN[2]))
	screen.drawText(2,22,string.format("%.2f",PN[3]))
end
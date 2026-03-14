pos={x=0,y=0,z=0}
Tpos={x=0,y=0,z=0}

function onTick()
	GN=input.getNumber

	dpos={x=GN(1)-pos.x,y=GN(2)-pos.y,z=GN(3)-pos.z}
	dTpos={x=GN(7)-Tpos.x,y=GN(8)-Tpos.y,z=GN(9)-Tpos.z}

	pos={x=GN(1),y=GN(2),z=GN(3)}
	rol,pit,yaw=GN(4),GN(5),GN(6)
	Tpos={x=GN(7),y=GN(8),z=GN(9)}
end

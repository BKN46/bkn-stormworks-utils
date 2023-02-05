from enum import Enum
from typing import List

class Port(Enum):
    NUM = 1
    BOOL = 2
    COMP = 3
    VIDEO = 4
    AUDIO = 5


class Module:
    def __init__(
        self,
        name,
        alias: str = "",
        inport: List[Port]=[],
        outport: List[Port]=[],
    ) -> None:
        self.name = name
        self.alias = alias
        self.inport = inport
        self.outport = outport
        self.width = 1
        self.length = max(0.25 + max(len(outport), len(inport)) * 0.25, 0.5)


COMPONENT_LIB = [
    Module("NOT"),
    Module("AND"),
    Module("OR"),
    Module("XOR"),
    Module("NAND"),
    Module("NOR"),
    Module("Add"),
    Module("Substract"),
    Module("Multiply"),
    Module("Divide"),
    Module("f(x,y,z)"),
    Module("Clamp"),
    Module("Threshold"),
    Module("Memory Register"),
    Module("Abs"),
    Module("Constant Number"),
    Module("Constant On Signal"),
    Module("Greater Than"),
    Module("Less Than"),
    Module("Property Slider"),
    Module("Property Dropdown"),
    Module("Numerical Junction"),
    Module("Numerical Switchbox"),
    Module("PID Controller"),
    Module("SR Latch"),
    Module("JK Flip Flop"),
    Module("Capacitor"),
    Module("Blinker"),
    Module("Push To Toggle"),
    Module("Composite Read (on/off)"),
    Module("Composite Read (number)"),
    Module("Property Toggle"),
    Module("Property Number"),
    Module("Delta"),
    Module("f(x,y,z,a,b,c,d)"),
    Module("Up/Down Counter"),
    Module("Modulo (fmod)"),
    Module("PID Controller (Advanced)"),
    Module("Composite Write (number)"),
    Module("Composite Write (on/off)"),
    Module("Equal"),
    Module("Tooltip Number"),
    Module("Tooltip On/Off"),
    Module("f(x)"),
    Module("Boolean f(x,y,z,w)"),
    Module("Boolean f(x,y,z,w,a,b,c,d)"),
    Module("Pulse (Toggle to Push)"),
    Module("Timer (TON)"),
    Module("Timer (TOF)"),
    Module("Timer (RTO)"),
    Module("Timer (RTF)"),
    Module("Composite Switchbox"),
    Module("Number To Composite Binary"),
    Module("Composite Binary To Number"),
    Module("Lua Script", alias="Lua", inport=[Port.COMP, Port.VIDEO], outport=[Port.COMP, Port.VIDEO]),
    Module("Video Switchbox"),
    Module("Property Text"),
    Module("Audio Switchbox"),
]

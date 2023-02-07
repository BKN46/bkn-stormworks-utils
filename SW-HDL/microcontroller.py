import math
from typing import List, Tuple

from modules import COMPONENT_LIB


MC_BASE = """
<?xml version="1.0" encoding="UTF-8"?>
<microprocessor name="##NAME##" description="##DESC##" width="##WIDTH##" length="##LENGTH##" id_counter="##NUM_COMP##" id_counter_node="##NUM_NODE##">
	<nodes>
##NODES##
	</nodes>
	<group>
		<data type="-139198522">
			<inputs/>
			<outputs/>
		</data>
		<components>
##COMP##
		</components>
		<components_bridge>
##BRIDGE##
		</components_bridge>
		<groups/>
		<component_states>
##COMP_STATE##
		</component_states>
		<component_bridge_states>
##BRIDGE_STATE##
		</component_bridge_states>
		<group_states/>
	</group>
</microprocessor>
"""


def tab(num=3):
    return "\t" * num


class Node:
    def __init__(
        self,
        id: int,
        node_type: int,
        component_id: int,
        mode: int = 0,
        label: str = "label",
        desc: str = "",
    ) -> None:
        self.id = id
        self.component_id = component_id
        self.node_type = node_type
        self.mode = mode
        self.label = label
        self.desc = desc
        self.pos = [0, 0]

    def get_xml(self):
        return (
            f'{tab(2)}<n id="{self.id}" component_id="{self.component_id}">\n'
            f'{tab(3)}<node label="{self.label}" mode="{self.mode}" type="{self.node_type}" description="{self.desc}">\n'
            f'{tab(4)}<position x="{self.pos[0]}" z="{self.pos[1]}"/>\n'
            f"{tab(3)}</node>\n"
            f"{tab(2)}</n>"
        )


class Component:
    def __init__(self, module: str, comp_id: int) -> None:
        mod_find = [
            (i, x)
            for (i, x) in enumerate(COMPONENT_LIB)
            if x.name == module or x.alias == module
        ]
        if not mod_find:
            raise Exception(f"No module named or aliased {module}")
        self.mod_id, self.mod = mod_find[0]
        self.id = comp_id
        self.input: List[Tuple[int, int] | None] = [None for _ in range(len(self.mod.inport))]
        self.pos = (0, 0)
        self.attr = {}
        self.value = {}

    def set_object_attr(self, **kwargs):
        self.attr = kwargs

    def set_value(self, **kwargs):
        self.value = kwargs

    def set_input(self, port: int, src_id: int, src_port: int):
        if port < len(self.input):
            self.input[port] = (src_id, src_port)
        else:
            raise Exception(f"No input port {port} for {self.mod.name}")

    def get_attr_str(self, attr: dict):
        if attr:
            return " " + " ".join([f'{k}="{v}"' for k, v in self.attr.items()])
        return ""

    def get_value_str(self, tab_num: int = 5):
        if self.value:
            return (
                tab(tab_num)
                + f"\n{tab(tab_num)}".join(
                    [f"<{k}{self.get_attr_str(v)}/>" for k, v in self.value.items()]
                )
                + "\n"
            )
        return ""

    def get_input_node_str(self, tab_num: int = 5):
        if self.input:
            return (
                tab(tab_num)
                + f"\n{tab(tab_num)}".join(
                    [
                        f'<in{i+1} component_id="{x[0]}" node_index="{x[1]}"/>'
                        for i, x in enumerate(self.input) if x
                    ]
                )
                + "\n"
            )
        return ""

    def get_xml(self):
        return (
            f'{tab(3)}<c type="{self.mod_id}">\n'
            f'{tab(4)}<object id="{self.id}"{self.get_attr_str(self.attr)}>\n'
            f'{tab(5)}<pos x="{self.pos[0]}" y="{self.pos[1]}"/>\n'
            f"{self.get_input_node_str()}"
            f"{tab(4)}</object>\n"
            f"{tab(3)}</c>"
        )

    def get_state_xml(self, cid=0):
        return (
            f'{tab(3)}<c{cid} id="{self.id}"{self.get_attr_str(self.attr)}>\n'
            f'{tab(4)}<pos x="{self.pos[0]}" y="{self.pos[1]}"/>\n'
            f"{self.get_input_node_str(tab_num=4)}"
            f"{tab(3)}</c>"
        )


class Microcontroller:
    def __init__(
        self, name: str = "SW HDL Microcontroller", desc: str = "No desc."
    ) -> None:
        self.name = name
        self.desc = desc
        self.nodes: List[Node] = []
        self.components: List[Component] = []
        self.bridges: List[Component] = []

    def get_id(self) -> int:
        return len(self.components) + len(self.bridges) + 1

    def add_comp(self, module: str) -> int:
        comp_id = self.get_id()
        new_comp = Component(module, comp_id)
        self.components.append(new_comp)
        return comp_id

    def add_node(
        self,
        node_type: int = 0,
        node_mode: int = 0,
        label: str = "label",
        desc: str = "",
    ) -> int:
        node_id = self.get_id()
        new_node = Node(len(self.nodes) + 1, node_type, node_id, mode=node_mode, label=label, desc=desc)
        new_bridge = Component("", node_id)
        self.nodes.append(new_node)
        self.bridges.append(new_bridge)
        return node_id

    def get_xml(self):
        width = math.ceil(math.sqrt(len(self.nodes)))
        length = math.ceil(len(self.nodes) / width)

        res = MC_BASE\
                .replace("##NAME##", self.name)\
                .replace("##DESC##", self.desc)\
                .replace("##WIDTH##", str(width))\
                .replace("##LENGTH##", str(length))\
                .replace("##NUM_COMP##", str(self.get_id() - 1))\
                .replace("##NUM_NODE##", str(len(self.nodes)))\
                .replace("##NODES##", "\n".join([x.get_xml() for x in self.nodes]))\
                .replace("##COMP##", "\n".join([x.get_xml() for x in self.components]))\
                .replace("##BRIDGE##", "\n".join([x.get_xml() for x in self.bridges]))\
                .replace("##COMP_STATE##", "\n".join([x.get_state_xml() for x in self.components]))\
                .replace("##BRIDGE_STATE##", "\n".join([x.get_state_xml() for x in self.bridges]))
        return res


if __name__ == "__main__":
    mc = Microcontroller()
    mc.add_node()
    mc.add_comp("Lua")
    res = mc.get_xml()
    print(res)

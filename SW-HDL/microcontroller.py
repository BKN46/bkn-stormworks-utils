from typing import List

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


class Node:
    pass


class Component:
    attr = {}

    def __init__(self, module, comp_id) -> None:
        mod_find = [
            (i, x) for (i, x) in enumerate(COMPONENT_LIB) if x.name == module or x.alias == module
        ]
        if not mod_find:
            raise Exception(f"No module named or aliased {module}")
        self.mod_id, self.mod = mod_find[0]
        self.id = comp_id
        self.pos = (0, 0)

    def set_object_attr(self, **kwargs):
        self.attr = kwargs

    def get_comp_xml(self, tab_num=3):
        tab = '\t'
        obj_attr = " ".join([
            f"{k}=\"{v}\"" for k, v in self.attr.items()
        ])

        return (
            f"{tab*tab_num}<c type=\"{self.mod_id}\">\n"
            f"{tab*tab_num + tab}<object id=\"{self.id}\" {obj_attr}>\n"
            f"{tab*tab_num + tab * 2}<pos x=\"{self.pos[0]}\" y=\"{self.pos[1]}\"/>\n"
            f"{tab*tab_num + tab}</object>"
            f"{tab*tab_num}</c>"
        )


class Bridge:
    pass


class Microcontroller:
    nodes: List[Node] = []
    components: List[Component] = []
    bridges: List[Bridge] = []

    def add_node(self):
        pass

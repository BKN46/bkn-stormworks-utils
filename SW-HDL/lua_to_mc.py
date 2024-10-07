import os
import tempfile
import time
import lupa

from microcontroller import Microcontroller


def get_lua_io(script):
    lua = lupa.LuaRuntime(unpack_returned_tuples=True)
    prefix_block = '''
    input = {}
    output = {}

    rec_in_bool={}
    rec_out_bool={}
    rec_in_num={}
    rec_out_num={}

    function input.getNumber(index)
        rec_in_num[index]=true
        return 0
    end
    function input.getBool(index)
        rec_in_bool[index]=true
        return false
    end
    function output.setNumber(index, value)
        rec_out_num[index]=true
    end
    function output.setBool(index, value)
        rec_out_bool[index]=true
    end
    function onTick() end
    '''

    lua.execute(prefix_block)
    lua.execute(script)
    lua.globals().onTick()
    res = (
        list(lua.globals().rec_in_bool),
        list(lua.globals().rec_out_bool),
        list(lua.globals().rec_in_num),
        list(lua.globals().rec_out_num),
    )
    return res


def get_lua_xml(script):
    mc = Microcontroller()
    component_lua = mc.add_comp("Lua Script")
    component_lua.attr['script'] = script

    lua_io = get_lua_io(script)
    num_write, bool_write = None, None

    for x in lua_io[0]:
        if not bool_write:
            bool_write = mc.add_comp("Composite Write (on/off)")
            component_lua.set_input(0, bool_write.id, 0)
        bool_in = mc.add_node(label=f'input bool {x}', node_type=0, node_mode=1)
        bool_write.set_input(x, bool_in.id, 0)

    for x in lua_io[2]:
        if not num_write:
            num_write = mc.add_comp("Composite Write (number)")
            if bool_write:
                bool_write.set_input(0, num_write.id, 0)
            else:
                component_lua.set_input(0, num_write.id, 0)

        num_in = mc.add_node(label=f'input num {x}', node_type=1, node_mode=1)
        num_write.set_input(x, num_in.id, 0)

    for x in lua_io[1]:
        bool_out = mc.add_node(label=f'output bool {x}', node_type=0, node_mode=0)
        bool_read = mc.add_comp("Composite Read (on/off)")
        bool_read.set_input(0, component_lua.id, 0)
        bool_read.attr['i'] = f"{x-1}"
        bool_out.set_input(0, bool_read.id, 0)

    for x in lua_io[3]:
        num_out = mc.add_node(label=f'output num {x}', node_type=1, node_mode=0)
        num_read = mc.add_comp("Composite Read (number)")
        num_read.set_input(0, component_lua.id, 0)
        num_read.attr['i'] = f"{x-1}"
        num_out.set_input(0, num_read.id, 0)


    res = mc.get_xml()
    return res


def lua_minify(lua_code):
    with tempfile.NamedTemporaryFile(delete=True, dir="temp/", prefix=f"{int(time.time() * 1000)}_", suffix=".lua") as f:
        f.write(lua_code.encode())
        f.seek(0)
        os.system(f"luamin -f {f.name} > {f.name}.min")
        with open(f"{f.name}.min", "r") as f2:
            min_code = f2.read()
        os.remove(f"{f.name}.min")
        return min_code


if __name__ == "__main__":
    script = '''
function onTick()
    IN=input.getNumber
    IB=input.getBool
    ON=output.setNumber
    OB=output.setBool
    IN(1)
    IB(6)
    IN(2)
    ON(3,1)
    OB(3,true)
end
'''
    get_lua_xml(script)

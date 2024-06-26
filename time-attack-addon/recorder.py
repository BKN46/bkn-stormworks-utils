from json import dump
import os
import time

from flask import Flask, request
from requests import post
from config import SERVER_HOST

app = Flask(__name__)

DATA = []
NOW_SESSION = None
PLAYER, STEAM_ID, COST = "", "", -1
START_TIME, END_TIME  = 0, 0

@app.route("/start")
def get_start():
    '''
        player: 玩家名
        steam_id: 玩家steam id
    '''
    global NOW_SESSION, DATA, PLAYER, START_TIME, STEAM_ID, COST
    PLAYER = request.args.get("player") # type: ignore
    STEAM_ID = request.args.get("steam_id")
    COST = int(request.args.get("cost") or -1)
    NOW_SESSION = time.strftime("%Y_%m_%d_%H_%M_%S", time.localtime())
    DATA = []
    START_TIME = time.time()
    open(f"tmp.tsv", "w").write("x\ty\tz\ttimer\textra\tmsg\n")
    print(f"开始计时, 玩家: {PLAYER}, steam_id: {STEAM_ID}, 车辆费用: {COST}")
    return "start:done"

@app.route("/send")
def get_info():
    '''
        [x, y, z, timer, extra, msg]
    '''
    value = request.args.get("value").replace('|||', '\n') # type: ignore
    open(f"tmp.tsv", "a").write("\n".join([x.replace(",", "\t") for x in value.split("\n")]) + "\n")
    DATA.extend([x.split(",") for x in value.split("\n")])
    for line in value.split("\n"):
        if "Point " in line:
            index = line.split("Point ")[1]
            print(f"到达检查点-{index}")
    return "send:done"


@app.route("/ping")
def get_ping():
    print("收到ping")
    try:
        res = post(f"http://{SERVER_HOST}/ping")
        if res.json():
            print(f"收到返回: {res.text}")
            return "ping:yeah"
    except Exception as e:
        pass
    return "ping:nah"


@app.route("/end")
def get_end():
    '''
        time: 用时
    '''
    global NOW_SESSION, END_TIME
    print("完成比赛！")
    END_TIME = time.time()
    use_time = request.args.get("time") or 0 # type: ignore
    auto_save_path = os.path.join(find_sw_path(), "tmp\\autosave.xml")
    if os.path.exists(auto_save_path):
        vehicle_data = open(auto_save_path, "rb").read().hex()
        vehicle_png = open(auto_save_path.replace(".xml", ".png"), "rb").read().hex()
    else:
        vehicle_data = ""
        vehicle_png = ""
    data = {
        "data": DATA,
        "player": PLAYER,
        "steam_id": STEAM_ID,
        "cost": COST,
        "time": NOW_SESSION,
        "back_start_time": START_TIME,
        "back_end_time": END_TIME,
        "use_time": use_time,
        "human_time": tick_to_time(int(use_time)),
        "vehicle_data": vehicle_data,
        "vehicle_png": vehicle_png,
    }
    os.makedirs("./data", exist_ok=True)
    try:
        dump(data, open(f"./data/{NOW_SESSION}.sav", "w"), indent=4, ensure_ascii=False)
    except Exception as e:
        open(f"./data/{NOW_SESSION}_repr.sav", "w").write(repr(data))
    res = post(f"http://{SERVER_HOST}/record", json=data)
    print(f"成绩上传完成! 用时: {use_time}")
    return "upload:done"


def find_sw_path():
    user_dir = "C:\\Users\\"
    for path in os.listdir(user_dir):
        sw_dir = os.path.join(user_dir, path, "AppData\\Roaming\\Stormworks\\data\\vehicles")
        if os.path.isdir(sw_dir):
            return sw_dir
    return ""


def tick_to_time(tick):
    minute = tick // 3600
    second = (tick % 3600) // 60
    ms = int((tick % 60) / 60 * 1000)
    return f"{minute:02}:{second:02}.{ms:03}"


app.run(port=5588, debug=False, use_reloader=False)

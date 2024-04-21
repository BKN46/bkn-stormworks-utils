import json
import os

from flask import Flask, config, request
import requests

from config import MIRAI_HOST, QQ_GROUP, STEAM_WEB_API_KEY

app = Flask(__name__)

@app.route("/ping", methods=["POST"])
def get_ping():
    return {"code": 0, "msg": "ok"}

@app.route("/record", methods=["POST"])
def get_record():
    data = request.get_json()
    os.makedirs("record", exist_ok=True)
    with open(f"record/{data['steam_id']}_{data['time']}_{data['use_time']}.json", "w") as f:
        json.dump(data, f)

    player_name = get_steam_name(data["steam_id"])

    if not os.path.exists("record/total.tsv"):
        open("record/total.tsv", "w").write("steam_id\tname\ttime\tuse_time(ms)\tcost\n")
    with open("record/total.tsv", "a") as f:
        f.write(f"{data['steam_id']}\t{player_name}\t{data['time']}\t{data['use_time']}\t{data['cost']}\n")

    requests.post(f"http://{MIRAI_HOST}/send", json={
        "group": QQ_GROUP,
        "message": f"直升机竞速挑战赛\n{player_name}({data['steam_id']})做出有效成绩: {tick_to_time(int(data['use_time']))}",
    })
    return {"code": 0, "msg": "ok"}

def tick_to_time(tick):
    minute = tick // 3600
    second = (tick % 3600) // 60
    ms = int((tick % 60) / 60 * 1000)
    return f"{minute:02}:{second:02}.{ms:03}"

def get_steam_name(steam_id):
    url = f"https://api.steampowered.com/ISteamUser/GetPlayerSummaries/v0002/?steamids={steam_id}&key={STEAM_WEB_API_KEY}"
    res = requests.get(url)
    try:
        return res.json()["response"]["players"][0]["personaname"]
    except Exception as e:
        return ""

app.run(host="0.0.0.0", port=5432, debug=True, use_reloader=False)

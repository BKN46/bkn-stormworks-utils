import os
import msvcrt

from flask import Flask, request

app = Flask(__name__)
BASE_DIR = os.path.dirname(__file__)
DATA_FILE = os.path.join(BASE_DIR, "data.csv")
LOCK_FILE = os.path.join(BASE_DIR, "data.csv.lock")

@app.route("/send")
def get_info():
    value = request.args.get("value")
    value = value.replace("|||", "\n")  # 将分隔符替换为换行符
    with open(LOCK_FILE, "a+b") as lock_fp:
        lock_fp.seek(0)
        msvcrt.locking(lock_fp.fileno(), msvcrt.LK_LOCK, 1)
        try:
            with open(DATA_FILE, "a", encoding="utf-8") as data_fp:
                data_fp.write(f"{value}\n")
        finally:
            lock_fp.seek(0)
            msvcrt.locking(lock_fp.fileno(), msvcrt.LK_UNLCK, 1)
    return "done"

app.run(host="0.0.0.0", port=5588)

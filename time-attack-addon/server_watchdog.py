import datetime
import flask

app = flask.Flask(__name__)

@app.route("/heartbeat", methods=["GET"])
def get_ping():
    params = flask.request.args
    now_time_str = datetime.datetime.now().strftime("%Y-%m-%d %H:%M:%S")
    return f"announce||Heartbeat|Heartbeat done in {now_time_str}, TPS: {float(params.get('tps', 0.0)):.2f}"

app.run(host="0.0.0.0", port=5588, debug=True, use_reloader=False)

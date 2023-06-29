import threading

from flask import Flask, request
from werkzeug.serving import make_server

from render import Oscilloscope

class ServerThread(threading.Thread):
    def __init__(self, app, port):
        threading.Thread.__init__(self)
        self.port = port
        self.server = make_server('127.0.0.1', port, app)
        self.ctx = app.app_context()
        self.ctx.push()

    def run(self):
        self.server.serve_forever()

    def shutdown(self):
        print(f"shutting down server on port {self.port}")
        self.server.shutdown()


app = Flask(__name__)
processes = []
record = None

# data split by comma
@app.route("/send")
def get_info():
    value = request.args.get("value")
    if record:
        values = [float(x) for x in str(value).split(",")]
        record.update(values)
    else:
        print(f"{value}",file=open("data.csv", "a"))
    return "done"

def shutdown_server():
    for process in processes:
        process.shutdown()

def run_server(port):
    # app.run(port=port, debug=False, use_reloader=False)
    server = ServerThread(app, port)
    server.start()
    processes.append(server)


if __name__ == "__main__":
    record = Oscilloscope()
    server = ServerThread(app, 5588)
    server.start()

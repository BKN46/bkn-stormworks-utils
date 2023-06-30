import os
import sys

# if __name__ != "__main__":
#     import matplotlib
#     matplotlib.use('Agg')

from matplotlib import pyplot as plt
import numpy as np

ON_PLOTTING = True

def on_key(event):
    global ON_PLOTTING
    if event.key == "q" or event.key == "escape":
        ON_PLOTTING = False
        plt.ioff()
        plt.close()

class Oscilloscope:
    def __init__(self, show_limit=300, channels=32, monitor_windows=None, bool_mode=0):
        fig = plt.figure(figsize=(12, 6), dpi=80)
        fig.canvas.mpl_connect('key_press_event', on_key)
        plt.ion()
        self.record, self.time = [[0 for _ in range(show_limit)] for _ in range(channels)], 0
        self.show_limit = show_limit
        self.update([])
        self.monitor_windows = None
        self.bool_mode = bool_mode != 0

    def update(self, data: list):
        for line in data:
            for index, tmp in enumerate(line):
                if self.bool_mode:
                    tmp += index
                self.record[index].append(tmp)
                self.record[index] = self.record[index][-self.show_limit:]
        plt.cla()
        plt.title(f"sw-oscilloscope (every {self.show_limit} ticks)")
        plt.grid(True)
        plt.xlabel("time(ticks)")
        if self.show_limit <= 60:
            plt.xticks(np.arange(1, self.show_limit + 1, 1))
        elif self.show_limit <= 100:
            plt.xticks(np.arange(1, self.show_limit + 1, 10))
        else:
            plt.xticks(np.arange(1, self.show_limit + 1, 30))
        plt.ylabel("value")
        for index, line in enumerate(self.record):
            if not any(line):
                continue
            plt.plot(line, label=f"channel {index}")
        plt.legend(loc="upper left")
        plt.pause(0.5)

    def close(self):
        plt.cla()
        plt.ioff()
        plt.close()
        # plt.show()

    @staticmethod
    def active_update(file_path, show_limit=300, bool_mode=0):
        osc = Oscilloscope(channels=64, show_limit=show_limit, bool_mode=bool_mode)
        start_line = 0
        while ON_PLOTTING:
            # time.sleep(time_interval)
            if not os.path.exists(file_path):
                osc.update([])
                continue
            raw_data = open(file_path, 'r').readlines()
            data = [[float(x or 0) for x in line.split(',')] for line in raw_data[start_line:]]
            osc.update(data)
            start_line = len(raw_data)
        osc.close()


if __name__ == "__main__":
    data_path = input("Please input the data path(default to 'data.csv'): ") or "data.csv"
    show_limit = int(input("Please input the show limit(default to 300): ") or "300")
    bool_mode = int(input("Please input the mode(default to 0): ") or 0)
    Oscilloscope.active_update(data_path, show_limit=show_limit, bool_mode=bool_mode)

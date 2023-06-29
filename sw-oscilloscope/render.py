import os
import random
import sys
import time

if __name__ != "__main__":
    import matplotlib
    matplotlib.use('Agg')

from matplotlib import pyplot as plt
import numpy as np


class Oscilloscope:
    def __init__(self, show_limit=300, channels=32, monitor_windows=None):
        plt.figure(figsize=(12, 6), dpi=80)
        plt.ion()
        self.record, self.time = [[0 for _ in range(show_limit)] for _ in range(channels)], 0
        self.show_limit = show_limit
        self.update([])
        self.monitor_windows = None

    def update(self, data: list):
        if not self.monitor_windows:
            for line in data:
                for index, tmp in enumerate(line):
                    self.record[index].append(tmp)
                    self.record[index] = self.record[index][-self.show_limit:]
            plt.cla()
            plt.title(f"sw-oscilloscope (every {self.show_limit} ticks)")
            plt.grid(True)
            plt.xlabel("time(ticks)")
            plt.xticks(np.arange(0, self.show_limit + 1, 30))
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
    def active_update(file_path, time_interval=0.5):
        osc = Oscilloscope()
        start_line = 0
        while True:
            # time.sleep(time_interval)
            if not os.path.exists(file_path):
                continue
            raw_data = open(file_path, 'r').readlines()
            data = [list(map(float, line.split(','))) for line in raw_data[start_line:]]
            osc.update(data)
            start_line = len(raw_data)


if __name__ == "__main__":
    if len(sys.argv) > 1:
        Oscilloscope.active_update(sys.argv[1])
    else:
        Oscilloscope.active_update("data.csv")

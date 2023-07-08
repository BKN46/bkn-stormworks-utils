import tkinter as tk

import receiver

def start_record():
    if button1_text.get() == "停止记录":
        button1_text.set("开始记录")
        receiver.shutdown_server()
        alert_text.set(f"已停止监听端口{port_entry_content.get()}")
    else:
        try:
            button1_text.set("停止记录")
            receiver.record = pure_monitor.get()
            receiver.run_server(port = int(port_entry_content.get()))
            alert_text.set(f"已开始监听端口{port_entry_content.get()}, 日志输出到data.csv")
        except Exception as e:
            alert_text.set(f"错误：{e}")


windows = tk.Tk()
windows.title("sw-oscilloscope")
windows.geometry("320x100")


DO_MONITOR = False

def on_closing():
    receiver.shutdown_server()
    windows.destroy()

windows.protocol("WM_DELETE_WINDOW", on_closing)

tk.Label(windows, text="监听端口").grid(row=0, column=0, padx=10)
port_entry_content = tk.StringVar(value="5588")
port_entry = tk.Entry(windows,textvariable=port_entry_content)
port_entry.grid(row=0, column=1, padx=10)
save_path = tk.StringVar(value="data.csv")
tk.Label(windows, text="保存路径").grid(row=1, column=0, padx=10)
tk.Entry(windows, textvariable=save_path).grid(row=1, column=1, padx=10)
button1_text = tk.StringVar(value="开始记录")
tk.Button(windows, textvariable=button1_text, command=start_record).grid(row=2, column=0, padx=10)
pure_monitor = tk.BooleanVar(value=False)
tk.Checkbutton(windows, text="纯监控", variable=pure_monitor).grid(row=2, column=1, padx=10)
# tk.Button(windows, text="开启监控", command=start_monitor).grid(row=2, column=1, padx=10)
# monitor_interval = tk.StringVar(value="300")
# tk.Entry(windows, textvariable=monitor_interval).grid(row=2, column=1, padx=10)
alert_text = tk.StringVar(value="")
tk.Label(windows, textvariable=alert_text).grid(row=3, column=0, columnspan=2, padx=10)

windows.mainloop()

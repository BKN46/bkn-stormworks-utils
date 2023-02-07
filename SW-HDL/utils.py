import os

def find_sw_path():
    user_dir = "C:\\Users\\"
    for path in os.listdir(user_dir):
        sw_dir = os.path.join(user_dir, path, "AppData\\Roaming\\Stormworks\\data\\microcontrollers")
        if os.path.isdir(sw_dir):
            return sw_dir

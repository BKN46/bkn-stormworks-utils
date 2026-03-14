template = open("sample_data").read()
all_data = open("terrain_compressed.dat").read()
id_start_from = 8

split_interval = 4000
max_num = len(all_data) // split_interval
with open("terrain_data.xml", "w") as f:
    for i in range(max_num):
        data_str = all_data[i*split_interval:(i+1)*split_interval]
        cid = id_start_from + i
        print(template.format(cid=cid, id=i+1, data_str=data_str), file=f, end="")
import random
import requests

url = 'http://localhost:5588/send'
data = "\n".join([f"{random.gauss(0,5) + 30},{random.gauss(0,10) + 50},{random.gauss(0,1) + 90}" for _ in range(30)])
res = requests.get(url, params={'value': data})
print(res.text)

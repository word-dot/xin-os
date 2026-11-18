#!/usr/bin/env python3
import requests
import json

url = "https://api.deepseek.com/anthropic/v1/messages"
headers = {
    "Authorization": "Bearer sk-1e53d960041f4c08bde1ea11b6450c0d",
    "Content-Type": "application/json",
    "anthropic-version": "2023-06-01",
    "User-Agent": "Claude-VSCode/2.1.158"
}
payload = {
    "model": "deepseek-v4-pro",
    "messages": [{"role": "user", "content": "Hello"}],
    "max_tokens": 50,
    "stream": False
}

try:
    resp = requests.post(url, headers=headers, json=payload, timeout=30)
    print(f"Status: {resp.status_code}")
    print(f"Headers: {dict(resp.headers)}")
    print(f"Body: {resp.text}")
except Exception as e:
    print(f"Error: {e}")
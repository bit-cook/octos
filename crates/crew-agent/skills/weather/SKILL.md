---
name: weather
description: Get current weather and forecasts using wttr.in (no API key required).
requires_bins: curl
---

# Weather

Get weather data using wttr.in (free, no API key needed).

## Quick Queries

```bash
# Current weather for a city
curl -s "wttr.in/Tokyo?format=3"
# Output: Tokyo: +15°C

# Detailed forecast
curl -s "wttr.in/London"

# One-line format
curl -s "wttr.in/Paris?format=%l:+%t+%C+%w"
# Output: Paris: +12°C Partly cloudy →11km/h
```

## Format Codes

| Code | Meaning |
|---|---|
| `%t` | Temperature |
| `%C` | Weather condition |
| `%w` | Wind |
| `%h` | Humidity |
| `%p` | Precipitation (mm) |
| `%l` | Location |
| `%S` | Sunrise |
| `%s` | Sunset |

## JSON Output

```bash
# Get JSON data
curl -s "wttr.in/Berlin?format=j1" | jq '.current_condition[0]'
```

## Fallback: Open-Meteo

If wttr.in is unavailable, use Open-Meteo (free, no key):

```bash
# Get weather by coordinates (latitude, longitude)
curl -s "https://api.open-meteo.com/v1/forecast?latitude=35.68&longitude=139.69&current_weather=true"
```

## Tips

- wttr.in supports city names, airport codes (e.g., `JFK`), and coordinates
- Add `?lang=zh` for Chinese output, `?lang=ja` for Japanese
- Use `format=j1` for machine-readable JSON

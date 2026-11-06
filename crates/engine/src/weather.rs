//! 天氣：台灣縣市用內建座標查 Open-Meteo，不經 Claude、0 token。
use crate::local_cmd::strip;
use crate::pipeline::cn;
use regex::Regex;
use serde_json::Value;
use std::sync::LazyLock;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WeatherAsk {
    /// 0 今天、1 明天、2 後天
    pub day: usize,
    /// 句子裡講的地點，沒講就是 None。
    pub place: Option<String>,
}

pub struct City {
    pub name: &'static str,
    pub lat: f64,
    pub lon: f64,
}

/// 新竹、嘉義的縣和市共用一組座標，所以 22 縣市只有 20 筆。
const CITIES: &[City] = &[
    City { name: "台北", lat: 25.0375, lon: 121.5637 },
    City { name: "新北", lat: 25.0120, lon: 121.4650 },
    City { name: "基隆", lat: 25.1283, lon: 121.7419 },
    City { name: "桃園", lat: 24.9936, lon: 121.3010 },
    City { name: "新竹", lat: 24.8138, lon: 120.9675 },
    City { name: "苗栗", lat: 24.5602, lon: 120.8214 },
    City { name: "台中", lat: 24.1477, lon: 120.6736 },
    City { name: "彰化", lat: 24.0752, lon: 120.5447 },
    City { name: "南投", lat: 23.9096, lon: 120.6840 },
    City { name: "雲林", lat: 23.7117, lon: 120.5414 },
    City { name: "嘉義", lat: 23.4801, lon: 120.4491 },
    City { name: "台南", lat: 22.9997, lon: 120.2270 },
    City { name: "高雄", lat: 22.6273, lon: 120.3014 },
    City { name: "屏東", lat: 22.6762, lon: 120.4929 },
    City { name: "宜蘭", lat: 24.7570, lon: 121.7530 },
    City { name: "花蓮", lat: 23.9871, lon: 121.6015 },
    City { name: "台東", lat: 22.7583, lon: 121.1444 },
    City { name: "澎湖", lat: 23.5711, lon: 119.5793 },
    City { name: "金門", lat: 24.4367, lon: 118.3186 },
    City { name: "連江", lat: 26.1505, lon: 119.9499 },
];

const DAYS: &[(&str, usize)] = &[("今天", 0), ("今日", 0), ("明天", 1), ("明日", 1), ("後天", 2), ("后天", 2)];
const DAY_NAMES: [&str; 3] = ["今天", "明天", "後天"];

static ASK: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^(.{0,12}?)(天氣|天气)(如何|怎樣|怎麼樣|怎么样|好嗎|好吗)?$").unwrap());

/// 整句比對天氣問句；地點不一定是真的地名，由 `find_city` 把關。
pub fn parse_weather(text: &str) -> Option<WeatherAsk> {
    let t = strip(text);
    let c = ASK.captures(&t)?;
    let head = c.get(1)?.as_str();
    if head.is_empty() && c.get(3).is_none() {
        return None;
    }
    let (word, day) = DAYS.iter().find(|(w, _)| head.contains(w)).copied().unwrap_or(("", 0));
    let rest = if word.is_empty() { head.to_string() } else { head.replacen(word, "", 1) };
    let place = rest.trim_end_matches('的');
    Some(WeatherAsk { day, place: (!place.is_empty()).then(|| place.to_string()) })
}

/// 只認台灣縣市。不用 Open-Meteo 的地名查詢：它會把「高雄」查成四川，而且不會報錯。
pub fn find_city(name: &str) -> Option<&'static City> {
    let n = name.trim().replace('臺', "台").replace("馬祖", "連江");
    CITIES.iter().find(|c| n.starts_with(c.name))
}

pub fn forecast_url(city: &City) -> String {
    format!(
        "https://api.open-meteo.com/v1/forecast?latitude={}&longitude={}&current=temperature_2m\
&daily=weather_code,temperature_2m_max,temperature_2m_min,precipitation_probability_max\
&timezone=Asia%2FTaipei&forecast_days=3",
        city.lat, city.lon
    )
}

fn sky(code: u64) -> Option<&'static str> {
    Some(match code {
        0 => "晴",
        1 => "大致晴朗",
        2 => "多雲",
        3 => "陰",
        45 | 48 => "有霧",
        51 | 53 | 55 | 56 | 57 => "毛毛雨",
        61 | 80 => "小雨",
        63 | 66 | 81 => "中雨",
        65 | 67 | 82 => "大雨",
        71 | 73 | 75 | 77 | 85 | 86 => "下雪",
        95 => "雷雨",
        96 | 99 => "雷雨和冰雹",
        _ => return None,
    })
}

fn degrees(t: f64) -> String {
    let n = t.round() as i64;
    if n < 0 { format!("零下 {}", -n) } else { n.to_string() }
}

/// 把 Open-Meteo 的回覆變成一句話。少任何必要欄位都回錯誤，由呼叫端改問 Claude。
pub fn say_weather(city: &City, day: usize, json: &str) -> Result<String, String> {
    let v: Value = serde_json::from_str(json).map_err(|e| e.to_string())?;
    let at = |key: &str| v["daily"][key].get(day).cloned().unwrap_or(Value::Null);
    let num = |key: &str| at(key).as_f64().ok_or_else(|| format!("缺 {key}"));
    let code = at("weather_code").as_u64().ok_or("缺 weather_code")?;
    let sky = sky(code).ok_or_else(|| format!("不認得的天氣代碼 {code}"))?;
    let (high, low) = (num("temperature_2m_max")?, num("temperature_2m_min")?);
    let mut s = format!("{}{}{sky}，", city.name, DAY_NAMES[day]);
    if day == 0 {
        let now = v["current"]["temperature_2m"].as_f64().ok_or("缺目前氣溫")?;
        s += &format!("現在 {} 度，", degrees(now));
    }
    s += &format!("最高 {}、最低{} 度", degrees(high), if low.round() < 0.0 { degrees(low) } else { format!(" {}", degrees(low)) });
    if let Some(p) = at("precipitation_probability_max").as_u64() {
        s += &format!("，降雨機率{}", if p == 100 { "百分之百".to_string() } else { format!("百分之{}", cn(p as u32)) });
    }
    Ok(s + "。")
}

#[cfg(test)]
mod tests {
    use super::*;

    const JSON: &str = r#"{"current":{"temperature_2m":22.4,"weather_code":1},
        "daily":{"time":["2026-11-06","2026-11-07","2026-11-08"],"weather_code":[3,51,99],
        "temperature_2m_max":[26.3,25.1,20.0],"temperature_2m_min":[21.9,22.0,-1.4],
        "precipitation_probability_max":[4,29,null]}}"#;

    fn ask(day: usize, place: Option<&str>) -> Option<WeatherAsk> {
        Some(WeatherAsk { day, place: place.map(String::from) })
    }

    #[test]
    fn parse_weather_questions() {
        assert_eq!(parse_weather("今天天氣如何?"), ask(0, None));
        assert_eq!(parse_weather("天氣怎麼樣"), ask(0, None));
        assert_eq!(parse_weather("明天天氣如何"), ask(1, None));
        assert_eq!(parse_weather("後天的天氣"), ask(2, None));
        assert_eq!(parse_weather("台北明天天氣如何"), ask(1, Some("台北")));
        assert_eq!(parse_weather("明天高雄的天氣好嗎"), ask(1, Some("高雄")));
        assert_eq!(parse_weather("東京天氣如何"), ask(0, Some("東京")));
    }

    #[test]
    fn parse_weather_leaves_other_sentences_to_claude() {
        for t in ["這週末適合去爬山嗎", "今天天氣這麼好要做什麼", "幫我查天氣預報的網站", "天氣", "你覺得明天天氣會影響出貨嗎"] {
            assert_eq!(parse_weather(t), None, "{t}");
        }
    }

    #[test]
    fn find_city_only_knows_taiwan() {
        for (t, want) in [("台北", "台北"), ("臺北市", "台北"), ("台北市大安區", "台北"), ("新北", "新北"), ("高雄市", "高雄"), ("馬祖", "連江")] {
            assert_eq!(find_city(t).map(|c| c.name), Some(want), "{t}");
        }
        for t in ["東京", "這週末", "下週三", "", "台灣"] {
            assert!(find_city(t).is_none(), "{t}");
        }
    }

    #[test]
    fn taipei_is_not_in_sichuan() {
        let c = find_city("高雄").unwrap();
        assert!((22.0..23.5).contains(&c.lat) && (120.0..121.0).contains(&c.lon));
    }

    #[test]
    fn url_asks_for_three_days_in_taipei_time() {
        let u = forecast_url(find_city("台北").unwrap());
        assert!(u.starts_with("https://api.open-meteo.com/v1/forecast?latitude=25."), "{u}");
        assert!(u.contains("forecast_days=3") && u.contains("timezone=Asia%2FTaipei"), "{u}");
    }

    #[test]
    fn say_weather_today_has_current_temperature() {
        let c = find_city("台北").unwrap();
        assert_eq!(say_weather(c, 0, JSON).unwrap(), "台北今天陰，現在 22 度，最高 26、最低 22 度，降雨機率百分之四。");
    }

    #[test]
    fn say_weather_tomorrow_has_no_current_temperature() {
        let c = find_city("台北").unwrap();
        assert_eq!(say_weather(c, 1, JSON).unwrap(), "台北明天毛毛雨，最高 25、最低 22 度，降雨機率百分之二十九。");
    }

    #[test]
    fn say_weather_without_rain_chance_skips_it() {
        let c = find_city("台北").unwrap();
        assert_eq!(say_weather(c, 2, JSON).unwrap(), "台北後天雷雨和冰雹，最高 20、最低零下 1 度。");
    }

    #[test]
    fn bad_replies_are_errors_not_made_up_weather() {
        let c = find_city("台北").unwrap();
        for j in ["", "{}", r#"{"error":true,"reason":"limit"}"#, r#"{"daily":{"weather_code":[1234],"temperature_2m_max":[1.0],"temperature_2m_min":[1.0]}}"#] {
            assert!(say_weather(c, 0, j).is_err(), "{j}");
        }
    }
}

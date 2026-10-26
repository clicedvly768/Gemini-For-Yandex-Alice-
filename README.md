# Gemini, Spotify & Bandcamp Media Center (в т.ч. для Яндекс Станции)

Асинхронный высокопроизводительный сервис на Rust для интеграции голосового помощника **Яндекс Алиса** с искусственным интеллектом **Google Gemini**, плеером **Spotify** и поиском/воспроизведением независимой музыки с **Bandcamp**.

---

## 🛠 Сборка через Meson & Ninja

Проект полностью переведён на билд-систему **Meson** с бэкендом **Ninja** ([`meson.build`](file:///home/vptr/RustroverProjects/Gemini-for-alice/meson.build)):

```bash
# 1. Первичная настройка каталога сборки
meson setup builddir
```

### Доступные цели Ninja:

| Команда Ninja | Описание | Результат |
|---|---|---|
| `ninja -C builddir build-arm64-musl` | Сборка для **Яндекс Станции** (ARM64, Linux kernel 4.9 Musl) | `target/aarch64-unknown-linux-musl/release/Gemini-for-alice` (3.2 МБ) |
| `ninja -C builddir build-x86_64` | Сборка релиза для ПК / Сервера (x86_64 glibc) | `target/release/Gemini-for-alice` (3.9 МБ) |
| `ninja -C builddir build-x86_64-musl` | Сборка статичного x86_64 Musl релиза | `target/x86_64-unknown-linux-musl/release/` |
| `ninja -C builddir build-armv7-musl` | Сборка 32-битного ARM Musl | `target/armv7-unknown-linux-musleabihf/release/` |
| `ninja -C builddir build-all` | Сборка сразу под все 4 архитектуры | Все бинарники в `target/` |
| `ninja -C builddir run` | Локальный запуск сервера на ПК | Запуск на `http://0.0.0.0:8080/` |
| `ninja -C builddir push-station` | Заливка на Яндекс Станцию по ADB в `/tmp` | `adb push ... /tmp/gemini-alice` |

---

## 🚀 Возможности сервиса

1. **🎸 Проигрывание с Bandcamp**:
   - Поиск по каталогу Bandcamp через App Autocomplete API.
   - Автоматическое извлечение прямых потоков (MP3 128 kbps) из `TralbumData`.
   - Локальное воспроизведение через `ffplay`, `mpv` или `gst-launch-1.0` либо отдача прямого потока.
   - Голосовые команды:
     - *«Включи на Bandcamp Aphex Twin»*
     - *«Поставь на Bandcamp Drukqs»*
     - *«Останови Bandcamp»* / *«Что сейчас играет на Bandcamp?»*
   - REST API: `GET /api/bandcamp/play?q=...` и `GET /api/bandcamp/stop`.

2. **🎵 Spotify (Google Home Style)**:
   - *«Включи Queen»* / *«Поставь Bohemian Rhapsody»*
   - *«Лайкни этот трек»* / *«Мне нравится»*
   - *«Перемешай треки»* / *«Включи shuffle»*
   - *«Переключи музыку на телефон»* / *«Переведи на компьютер»*
   - *«Пауза»* / *«Продолжи»* / *«Следующий трек»* / *«Громкость 50»*

3. **🤖 Google Routines & Gemini Live**:
   - *«Доброе утро»* — структурированная утренняя сводка дня.
   - *«Спокойной ночи»* — сценарий подготовки ко сну.
   - *«Как устроен квантовый компьютер?»* — живой диалог с памятью контекста.

---

## 🌐 Быстрый запуск на ПК для тестов

```bash
ninja -C builddir run
```
И откройте в браузере: `http://localhost:8080/`

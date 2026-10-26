#!/bin/sh
# ==============================================================================
# Скрипт очистки Яндекс Станции (Lover/Lite/Mini/Midi/Max) от служб Quasar/Yandex
# ==============================================================================

set -e

echo "=== [1/5] Принудительная остановка процессов Яндекса ==="
# Основные демоны Яндекса: maind (Quasar), audiod, quasard, yandex-quasar, mediaserver
for proc in maind audiod quasard quasar yandex-quasar yastation; do
    if pgrep "$proc" >/dev/null 2>&1 || pidof "$proc" >/dev/null 2>&1; then
        echo "Остановка процесса: $proc"
        killall -9 "$proc" 2>/dev/null || pkill -9 "$proc" 2>/dev/null || true
    fi
done

echo "=== [2/5] Отключение автозапуска в init.d / SysVinit ==="
# В системах на Buildroot/OpenWrt службы находятся в /etc/init.d/
if [ -d /etc/init.d ]; then
    # Монтируем корень/system на запись, если файловая система read-only
    mount -o remount,rw / 2>/dev/null || true
    mount -o remount,rw /system 2>/dev/null || true

    # Отключение S99onboot или прямых скриптов запуска Quasar
    for init_script in /etc/init.d/S*quasar* /etc/init.d/S99onboot.sh /etc/init.d/*yandex*; do
        if [ -f "$init_script" ]; then
            echo "Обработка init-скрипта: $init_script"
            # Комментируем вызовы quasar/maind/yandex
            sed -i 's|.*vendor/quasar.*|# &|g' "$init_script" 2>/dev/null || true
            sed -i 's|.*maind.*|# &|g' "$init_script" 2>/dev/null || true
            sed -i 's|.*quasard.*|# &|g' "$init_script" 2>/dev/null || true
            # Или бэкапим и снимаем права на исполнение
            chmod -x "$init_script" 2>/dev/null || true
        fi
    done
fi

echo "=== [3/5] Отключение в systemd (если используется) ==="
if command -v systemctl >/dev/null 2>&1; then
    for svc in quasar quasard yandex-quasar yastation-maind; do
        if systemctl list-unit-files 2>/dev/null | grep -q "$svc"; then
            echo "Отключение systemd юнита: $svc"
            systemctl stop "$svc" 2>/dev/null || true
            systemctl disable "$svc" 2>/dev/null || true
            systemctl mask "$svc" 2>/dev/null || true
            rm -f "/etc/systemd/system/${svc}.service" 2>/dev/null || true
        fi
    done
    systemctl daemon-reload 2>/dev/null || true
fi

echo "=== [4/5] Отключение в Android init.rc (если AOSP/Android) ==="
if [ -f /system/etc/init/quasar.rc ] || [ -f /vendor/etc/init/quasar.rc ]; then
    echo "Обнаружен Android init.rc для quasar. Деактивация..."
    mount -o remount,rw /system 2>/dev/null || true
    mount -o remount,rw /vendor 2>/dev/null || true
    [ -f /system/etc/init/quasar.rc ] && mv /system/etc/init/quasar.rc /system/etc/init/quasar.rc.bak || true
    [ -f /vendor/etc/init/quasar.rc ] && mv /vendor/etc/init/quasar.rc /vendor/etc/init/quasar.rc.bak || true
fi

echo "=== [5/5] Запрет запуска бинарников и освобождение памяти ==="
QUASAR_DIR="/system/vendor/quasar"
if [ -d "$QUASAR_DIR" ]; then
    mount -o remount,rw /system 2>/dev/null || mount -o remount,rw / 2>/dev/null || true
    # Снимаем права исполнения с бинарников и скриптов
    chmod 000 "$QUASAR_DIR"/maind 2>/dev/null || true
    chmod 000 "$QUASAR_DIR"/run.sh 2>/dev/null || true
    chmod 000 "$QUASAR_DIR"/audiod 2>/dev/null || true
    echo "Права исполнения с $QUASAR_DIR успешно сняты."
fi

# Освобождаем кэш памяти
sync
echo 3 > /proc/sys/vm/drop_caches 2>/dev/null || true

echo "=== [✓] Готово! Все службы Яндекса и Квазара остановлены и отключены. ==="

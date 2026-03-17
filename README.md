# tg-ws-proxy-rs

Rust-реализация SOCKS5-прокси для Telegram с маршрутизацией Telegram MTProto-трафика через WebSocket endpoint'ы `kws*.web.telegram.org`.

Проект рассчитан на Linux и запуск в контейнере.

Поддерживаемые архитектуры контейнера:

- `linux/amd64`
- `linux/arm64`

## Что это

- Локальный или серверный `SOCKS5` прокси для Telegram-клиента.
- Для Telegram IPv4-трафика прокси пытается отправлять MTProto через `wss://kws*.web.telegram.org/apiws`.
- Для healthcheck поднимается отдельный HTTP endpoint.
- При старте приложение печатает `tg://socks?...` ссылку для добавления в Telegram.

## Текущее состояние

- Telegram IPv6-сессии сейчас не маршрутизируются через WS и обрабатываются отдельно.

## Требования

- Linux
- Docker 24+ или совместимый container runtime
- Для локальной сборки:
  - Rust 1.94+
  - Cargo 1.94+

## Установка Rust

```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
source ~/.cargo/env
rustup toolchain install 1.94
rustup default 1.94
```

## Локальный запуск

```bash
cargo run -- --host 0.0.0.0 --port 1080
```

Полезные параметры:

- `--host 0.0.0.0` слушать на всех интерфейсах
- `--port 1080` SOCKS5 порт
- `--log-level info|debug|warn`
- `--health-addr 0.0.0.0:8080`
- `--read-timeout 15`
- `--connect-timeout 10`

## Сборка и тесты

```bash
cargo fmt --all
cargo test
cargo build --release
```

На текущем этапе тесты покрывают:

- CLI конфигурацию
- генерацию proxy link
- WebSocket frame parsing
- MTProto message splitting
- routing/backoff логику
- TCP bridge

## Docker

Сборка образа:

```bash
docker build -t tg-ws-proxy-rs:latest .
```

Multi-arch сборка и публикация в `ghcr.io`:

```bash
docker buildx build \
  --platform linux/amd64,linux/arm64 \
  -t ghcr.io/evm-andrey/telegram-socks-ws-proxy:latest \
  -t ghcr.io/evm-andrey/telegram-socks-ws-proxy:0.1.0 \
  --push \
  .
```

Отдельный Dockerfile для `arm64` не нужен: текущий [Dockerfile](/home/evmenenko/Documents/gbunker/tproxy/Dockerfile) одинаково подходит для `amd64` и `arm64`, если собирать его через `docker buildx`.

Запуск контейнера:

```bash
docker run -d \
  --name tg-proxy-test \
  -p 1080:1080 \
  -p 8080:8080 \
  tg-ws-proxy-rs:latest
```

Проверка health endpoint:

```bash
curl -sf http://127.0.0.1:8080/health
```

Просмотр логов:

```bash
docker logs -f tg-proxy-test
```

Публичный образ:

```bash
docker pull ghcr.io/evm-andrey/telegram-socks-ws-proxy:latest
```

## Добавление в Telegram

При старте приложение пишет в лог ссылку вида:

```text
tg://socks?server=<host>&port=<port>
```

Пример:

```text
tg://socks?server=203.0.113.10&port=1080
```

Если приложение слушает на `0.0.0.0`, в логе будет:

```text
tg://socks?server=0.0.0.0&port=1080
```

Это служебное значение. Для реального клиента замените `0.0.0.0` на публичный IP или домен сервера.

## Транспорт

- Для Telegram IPv4-трафика используется только WebSocket маршрут.
- Для не-Telegram трафика остаётся обычный TCP passthrough.

## Как читать логи

Типичные полезные строки:

- `ws route selected dc=2 media=true -> kws2-1.web.telegram.org`
  - выбран WS маршрут для Telegram DC
- `ws bridge closed ... duration_ms=21231 up_bytes=53222 down_bytes=4598528 ...`
  - сессия реально передавала данные, указана длительность и объём трафика
- `telegram socks proxy link: tg://socks?...`
  - готовая ссылка для клиента

Поля завершения bridge:

- `up_end=client_eof`
  - локальный клиент первым закрыл TCP сторону
- `down_end=ws_eof`
  - WS сторона закрылась после завершения чтения
- `down_end=ws_close(code=...,reason=...)`
  - Telegram прислал явный WebSocket close frame

## Известные ограничения

- Telegram IPv6 сейчас не идёт через WS-маршрут.
- Часть клиентских соединений Telegram может быть короткоживущими probe-сессиями; это нормально и видно по коротким bridge в логах.
- В проекте пока нет полноценной метрики `/metrics`.
- В проекте нет аутентификации SOCKS5.

## Быстрая диагностика

Проверить статус контейнера:

```bash
docker ps --format '{{.Names}}\t{{.Status}}\t{{.Ports}}'
```

Посмотреть последние ошибки:

```bash
docker logs --tail 100 tg-proxy-test
```

Отфильтровать только проблемные строки:

```bash
docker logs tg-proxy-test 2>&1 | rg "WARN|ERROR|unknown dc|timeout|ws bridge err"
```

## Минимальный рабочий сценарий

1. Собрать образ.
2. Поднять контейнер на `0.0.0.0:1080`.
3. Взять `tg://socks?...` ссылку из логов.
4. Заменить `0.0.0.0` на внешний IP или домен.
5. Добавить прокси в Telegram как `SOCKS5`.
6. Проверить `docker logs -f tg-proxy-test` во время подключения.

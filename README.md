# tg-ws-proxy-rs

Rust-реализация SOCKS5-прокси для Telegram с маршрутизацией Telegram MTProto-трафика через WebSocket endpoint'ы `kws*.web.telegram.org`.

Проект рассчитан на Linux и запуск в контейнере.

Поддерживаемые архитектуры контейнера:

- `linux/amd64`
- `linux/arm64`

## Что это

- Локальный или серверный `SOCKS5` прокси для Telegram-клиента.
- Для Telegram IPv4-трафика прокси пытается отправлять MTProto через `wss://kws*.web.telegram.org/apiws`.
- Для Telegram IPv6-трафика прокси использует тот же WS-маршрут, если удалось извлечь или выучить `dc/media` для целевого IPv6-адреса.
- Для healthcheck поднимается отдельный HTTP endpoint.
- При старте приложение печатает `tg://socks?...` ссылки для bind-адреса или для найденных IPv4/IPv6 адресов, если слушает wildcard.

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
cargo run -- --host :: --port 1080
```

Полезные параметры:

- `--host ::` слушать сразу на `0.0.0.0` и `::`
- `--port 1080` SOCKS5 порт
- `SOCKS_USERS='alice:secret,bob:pass2'` включить SOCKS5 username/password auth для нескольких аккаунтов
- `--log-level info|debug|warn`
- `--health-addr [::]:8080`
- `--read-timeout 15`
- `--connect-timeout 10`

Пример запуска с авторизацией:

```bash
SOCKS_USERS='alice:secret,bob:pass2' cargo run -- --host :: --port 1080
```

Формат `SOCKS_USERS`:

- несколько учёток разделяются `,`, `;` или переводом строки
- каждая запись задаётся как `username:password`
- значение проверяется на старте; при некорректном формате приложение завершится с ошибкой

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

Сборка и публикация в `ghcr.io`:

```bash
docker buildx build \
  --platform linux/amd64,linux/arm64 \
  -t ghcr.io/evm-andrey/telegram-socks-ws-proxy:latest \
  -t ghcr.io/evm-andrey/telegram-socks-ws-proxy:0.1.3 \
  --push \
  .
```

Текущий [Dockerfile](/home/evmenenko/Documents/gbunker/tproxy/Dockerfile) собирается через `buildx` для `linux/amd64` и `linux/arm64`.

Запуск контейнера:

```bash
docker run -d \
  --name tg-proxy-test \
  -e SOCKS_USERS='alice:secret,bob:pass2' \
  -p 1080:1080 \
  -p 8080:8080 \
  tg-ws-proxy-rs:latest \
  --host ::
```

Если нужен bind на конкретном адресе, передайте его через `--host`, например:

```bash
docker run -d \
  --name tg-proxy-test \
  -e SOCKS_USERS='alice:secret,bob:pass2' \
  -p 1080:1080 \
  -p 8080:8080 \
  tg-ws-proxy-rs:latest \
  --host 192.168.1.254
```

Проверка health endpoint:

```bash
curl -sf http://127.0.0.1:8080/health
```

Проверка авторизации через SOCKS:

```bash
curl --proxy socks5h://alice:secret@127.0.0.1:1080 http://127.0.0.1:8080/health
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

IPv6-пример:

```text
tg://socks?server=[2001:db8::10]&port=1080
```

Если приложение слушает wildcard-адреса, оно попытается напечатать ссылки для обнаруженных интерфейсных IPv4/IPv6 адресов.
Если приложение уже слушает на конкретном адресе, ссылка строится из `--host`.

Если включена `SOCKS5` авторизация, логин и пароль нужно ввести в клиенте Telegram вручную. В `tg://socks?...` ссылка они не добавляются.

## Транспорт

- Для Telegram IPv4-трафика используется только WebSocket маршрут.
- Для Telegram IPv6-трафика используется тот же WebSocket маршрут, если для адреса известен `dc/media` из init или из runtime cache.
- WebSocket upgrade ответ от upstream теперь валидируется строго: `101`, `Upgrade`, `Connection`, `Sec-WebSocket-Accept` и `Sec-WebSocket-Protocol` должны совпадать с ожидаемым контрактом, иначе соединение отвергается до relay.
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

- Для части Telegram IPv6 first-hit всё ещё возможен direct passthrough, если `dc` нельзя извлечь из init и адрес ещё не выучен runtime cache.
- Часть клиентских соединений Telegram может быть короткоживущими probe-сессиями; это нормально и видно по коротким bridge в логах.
- В проекте пока нет полноценной метрики `/metrics`.

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
2. Поднять контейнер на `::/1080`, чтобы слушать оба стека.
3. Взять `tg://socks?...` ссылку из логов.
4. Если нужен конкретный внешний адрес, слушать сразу на нём через `--host <ipv4-or-ipv6>`.
5. Добавить прокси в Telegram как `SOCKS5`.
6. Проверить `docker logs -f tg-proxy-test` во время подключения.

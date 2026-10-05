FROM python:3.12-alpine
WORKDIR /app
COPY service.py /app/service.py
RUN addgroup -g 10001 relay && adduser -D -u 10001 -G relay relay && mkdir /data && chown relay:relay /data
USER 10001:10001
ENV PYTHONDONTWRITEBYTECODE=1 PYTHONUNBUFFERED=1 PUSH_DB=/data/events.sqlite PUSH_BIND=0.0.0.0
CMD ["python", "/app/service.py"]

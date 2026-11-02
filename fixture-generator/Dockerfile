# Fixed fixture-build environment (ADR-0001).
# Regenerate: docker build -t qt-fixgen . && docker run --rm -v $(pwd)/..:/work qt-fixgen
FROM ubuntu:24.04

ENV DEBIAN_FRONTEND=noninteractive
RUN apt-get update \
    && apt-get install -y --no-install-recommends \
        build-essential libquantlib0-dev \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /src
COPY fixgen.cpp Makefile ./
RUN make regen OUT_DIR=/out

# Default: regenerate into a mounted workspace volume at /work
CMD ["sh", "-c", "make regen OUT_DIR=/work/fixtures && echo 'regenerated; diff against git to verify'"]

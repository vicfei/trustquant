CXX ?= g++
CXXFLAGS ?= -O2 -std=c++17
OUT_DIR ?= ../fixtures

.PHONY: all regen clean

all: fixgen

fixgen: fixgen.cpp
	$(CXX) $(CXXFLAGS) -o $@ $< -lQuantLib

regen: fixgen
	mkdir -p $(OUT_DIR)/date_daycount $(OUT_DIR)/black_scholes_preview
	./fixgen --out-dir $(OUT_DIR)

clean:
	rm -f fixgen

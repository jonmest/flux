#!/bin/bash

# Color codes for output
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
RED='\033[0;31m'
NC='\033[0m' # No Color

echo -e "${BLUE}================================${NC}"
echo -e "${BLUE}Flux Load Balancer Benchmarks${NC}"
echo -e "${BLUE}================================${NC}"
echo ""

# Check if backends are running
echo -e "${YELLOW}Checking if backend servers are running on ports 3000 and 3001...${NC}"
if ! nc -z 127.0.0.1 3000 2>/dev/null; then
    echo -e "${RED}ERROR: No server on port 3000${NC}"
    echo "Please start backend: cargo run --release --bin echo_server 3000"
    exit 1
fi

if ! nc -z 127.0.0.1 3001 2>/dev/null; then
    echo -e "${RED}ERROR: No server on port 3001${NC}"
    echo "Please start backend: cargo run --release --bin echo_server 3001"
    exit 1
fi

echo -e "${GREEN}✓ Both backends are running${NC}"
echo ""

# Check if Flux is running
echo -e "${YELLOW}Checking if Flux is running on port 8080...${NC}"
if ! nc -z 127.0.0.1 8080 2>/dev/null; then
    echo -e "${RED}ERROR: Flux is not running on port 8080${NC}"
    echo "Please start Flux: cargo run --release"
    exit 1
fi

echo -e "${GREEN}✓ Flux is running${NC}"
echo ""

# Build all benchmarks
echo -e "${YELLOW}Building benchmarks in release mode...${NC}"
cargo build --release --bin benchmark --bin benchmark_pooled --bin benchmark_high_concurrency

if [ $? -ne 0 ]; then
    echo -e "${RED}Build failed!${NC}"
    exit 1
fi

echo -e "${GREEN}✓ Build complete${NC}"
echo ""

# Run original benchmark (not optimized for pooling)
echo -e "${BLUE}================================${NC}"
echo -e "${BLUE}Test 1: Original Benchmark${NC}"
echo -e "${BLUE}(Creates new connection per request)${NC}"
echo -e "${BLUE}================================${NC}"
./target/release/benchmark
echo ""

# Wait a bit between tests
sleep 2

# Run pooled connection benchmark
echo -e "${BLUE}================================${NC}"
echo -e "${BLUE}Test 2: Pooled Connection Benchmark${NC}"
echo -e "${BLUE}(Reuses connections - tests pooling)${NC}"
echo -e "${BLUE}================================${NC}"
./target/release/benchmark_pooled
echo ""

# Wait a bit between tests
sleep 2

# Run high concurrency benchmark
echo -e "${BLUE}================================${NC}"
echo -e "${BLUE}Test 3: High Concurrency Benchmark${NC}"
echo -e "${BLUE}(1000 workers - tests RwLock fix)${NC}"
echo -e "${BLUE}================================${NC}"
./target/release/benchmark_high_concurrency
echo ""

echo -e "${GREEN}================================${NC}"
echo -e "${GREEN}All benchmarks complete!${NC}"
echo -e "${GREEN}================================${NC}"
echo ""
echo -e "${YELLOW}Next steps:${NC}"
echo "1. Compare results between Test 1 and Test 2 (pooling improvement)"
echo "2. Test 3 shows benefit of RwLock fix under high concurrency"
echo "3. Setup nginx comparison: see nginx_setup.md"
echo ""

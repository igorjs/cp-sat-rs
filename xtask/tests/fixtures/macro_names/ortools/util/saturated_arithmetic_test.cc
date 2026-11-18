#define TEST_BINARY_OPERATOR(name, symbol, reference) \
  TEST_F(SaturatedArithmeticTest, name) {             \
    ASSERT_EQ(reference(1, 2), name(1, 2))            \
        << symbol;                                    \
  }

TEST_BINARY_OPERATOR(CapAdd, "+", ReferenceCapAdd);
TEST_BINARY_OPERATOR(CapSubGeneric, "-", ReferenceCapSub);
TEST_BINARY_OPERATOR(CapProd, "*", ReferenceCapProd);

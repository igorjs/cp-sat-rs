#define WRAPPED(name, op)            \
  TEST_F(Wrapped, name) {            \
    EXPECT_EQ(1, op);                \
  }

TEST(Model, A) {}
TEST(Model, B) {}
TEST_F(Fixture, C) {}
TEST_F(Fixture, D) {}
  TEST(Model, E) {}
TYPED_TEST(Typed, F) {}
TEST_P(Param, G) {}
TEST(Model, H) {}

WRAPPED(one, 1)
WRAPPED(two, 2)
WRAPPED(three, 3)
WRAPPED(four, 4)
WRAPPED(five, 5)
WRAPPED(six, 6)
WRAPPED(seven, 7)
WRAPPED(eight, 8)
WRAPPED(nine, 9)
